//! Fixed-capacity SDK event and parameter COM objects, allocated at activation.
use crate::processor::{BLOCK_EVENT_CAPACITY, EVENT_CAPACITY};
use std::cell::{Cell, UnsafeCell};
use std::rc::Rc;
use vst3::Steinberg::Vst::*;
use vst3::{Class, ComPtr, ComWrapper, Steinberg::*};

// Release each initially held channel/key once, then allow every admitted
// note-on both its native event and a later expanded panic release. Explicit
// reserved releases also need one slot each. Repeated panics cannot release
// the same held note twice without another admitted note-on in between.
// Recovery expressions and fixed-buffer collection share the host block bound.
// Each admitted note-on may later expand into a panic release, plus initially
// held keys/instances. Expressions themselves remain one pointer-free event.
const NOTE_CAPACITY: usize =
    16 * 128 + crate::events::MAX_NOTE_INSTANCES + 2 * BLOCK_EVENT_CAPACITY;

pub(super) const PENDING_PARAMETER_CAPACITY: usize = 256;
// All three input sources may be disjoint. Reserved instrument events cannot
// contain parameters. Neither points nor COM queues grow during a callback.
const INPUT_PARAMETER_CAPACITY: usize =
    PENDING_PARAMETER_CAPACITY + crate::instance::QUEUE_CAPACITY + BLOCK_EVENT_CAPACITY;

#[derive(Clone, Copy)]
struct Point {
    offset: i32,
    value: f64,
    next: Option<usize>,
}

struct Arena {
    points: UnsafeCell<Vec<Point>>,
    used: Cell<usize>,
    dropped: Cell<u32>,
}
impl Arena {
    fn new(capacity: usize) -> Rc<Self> {
        Rc::new(Self {
            points: UnsafeCell::new(vec![
                Point {
                    offset: 0,
                    value: 0.0,
                    next: None
                };
                capacity
            ]),
            used: Cell::new(0),
            dropped: Cell::new(0),
        })
    }
    fn drop_point(&self) {
        self.dropped.set(self.dropped.get().saturating_add(1));
    }
}
struct Queue {
    id: Cell<u32>,
    first: Cell<Option<usize>>,
    last: Cell<Option<usize>>,
    count: Cell<usize>,
    arena: Rc<Arena>,
}
impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}
#[allow(non_snake_case)]
impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> u32 {
        self.id.get()
    }
    unsafe fn getPointCount(&self) -> i32 {
        self.count.get() as i32
    }
    unsafe fn getPoint(&self, index: i32, offset: *mut i32, value: *mut f64) -> tresult {
        if index < 0 || index as usize >= self.count.get() || offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        let Some(mut at) = self.first.get() else {
            return kInvalidArgument;
        };
        // SAFETY: arena is used exclusively during this synchronous audio call;
        // its storage never grows, and links only point to allocated slots.
        let points = unsafe { &*self.arena.points.get() };
        for _ in 0..index {
            let Some(next) = points[at].next else {
                return kInvalidArgument;
            };
            at = next;
        }
        // SAFETY: SDK caller supplies output pointers, checked non-null.
        unsafe {
            offset.write(points[at].offset);
            value.write(points[at].value);
        }
        kResultOk
    }
    unsafe fn addPoint(&self, offset: i32, value: f64, index: *mut i32) -> tresult {
        if offset < 0 || !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return kInvalidArgument;
        }
        // SAFETY: audio-call-exclusive arena, fixed storage and bounded index.
        let points = unsafe { &mut *self.arena.points.get() };
        if let Some(last) = self.last.get() {
            if points[last].offset > offset {
                return kInvalidArgument;
            }
            // SDK queues have one value per offset. Main-thread edits,
            // deferred edits and automation may all target sample zero.
            if points[last].offset == offset {
                points[last].value = value;
                if !index.is_null() {
                    unsafe {
                        index.write(self.count.get() as i32 - 1);
                    }
                }
                return kResultOk;
            }
        }
        let at = self.arena.used.get();
        if at == points.len() {
            self.arena.drop_point();
            return kResultFalse;
        }
        if let Some(last) = self.last.get() {
            points[last].next = Some(at);
        } else {
            self.first.set(Some(at));
        }
        points[at] = Point {
            offset,
            value,
            next: None,
        };
        self.last.set(Some(at));
        self.arena.used.set(at + 1);
        // SAFETY: optional SDK output pointer.
        if !index.is_null() {
            unsafe {
                index.write(self.count.get() as i32);
            }
        }
        self.count.set(self.count.get() + 1);
        kResultOk
    }
}
struct Slot {
    queue: ComWrapper<Queue>,
    interface: ComPtr<IParamValueQueue>,
}
pub(super) struct Changes {
    queues: Vec<Slot>,
    active: Cell<usize>,
    arena: Rc<Arena>,
}
impl Changes {
    pub fn new() -> ComWrapper<Self> {
        Self::with_capacity(EVENT_CAPACITY, EVENT_CAPACITY)
    }
    pub fn input(parameter_count: usize) -> ComWrapper<Self> {
        Self::with_capacity(
            INPUT_PARAMETER_CAPACITY,
            parameter_count.min(INPUT_PARAMETER_CAPACITY),
        )
    }
    fn with_capacity(points: usize, queues: usize) -> ComWrapper<Self> {
        let arena = Arena::new(points);
        let queues = (0..queues)
            .map(|_| {
                let queue = ComWrapper::new(Queue {
                    id: Cell::new(0),
                    first: Cell::new(None),
                    last: Cell::new(None),
                    count: Cell::new(0),
                    arena: Rc::clone(&arena),
                });
                let interface = queue.to_com_ptr().expect("queue interface");
                Slot { queue, interface }
            })
            .collect();
        ComWrapper::new(Self {
            queues,
            active: Cell::new(0),
            arena,
        })
    }
    pub fn clear(&self) {
        for slot in self.queues.iter().take(self.active.get()) {
            slot.queue.first.set(None);
            slot.queue.last.set(None);
            slot.queue.count.set(0);
        }
        self.active.set(0);
        self.arena.used.set(0);
        self.arena.dropped.set(0);
    }
    pub fn push(&self, id: u32, offset: u32, value: f64) -> bool {
        let mut index = 0;
        let before = self.dropped();
        // SAFETY: synchronous call on exclusively owned COM objects and valid args.
        unsafe {
            let queue = self.addParameterData(&id, &mut index);
            if let Some(queue) = vst3::ComRef::from_raw(queue)
                && queue.addPoint(offset as i32, value, &mut index) == kResultOk
            {
                return true;
            }
        }
        if self.dropped() == before {
            self.arena.drop_point();
        }
        false
    }
    pub fn visit_final(&self, out: &mut dyn FnMut(u32, f64)) {
        // SAFETY: caller owns the fixed arena; the SDK process has returned.
        let points = unsafe { &*self.arena.points.get() };
        for slot in self.queues.iter().take(self.active.get()) {
            if let Some(last) = slot.queue.last.get() {
                out(slot.queue.id.get(), points[last].value);
            }
        }
    }
    pub fn visit(&self, out: &mut dyn FnMut(u32, u32, f64)) {
        // SAFETY: caller owns this arena; SDK calls have returned.
        let points = unsafe { &*self.arena.points.get() };
        for slot in self.queues.iter().take(self.active.get()) {
            let mut at = slot.queue.first.get();
            for _ in 0..slot.queue.count.get() {
                let Some(index) = at else {
                    break;
                };
                let point = points[index];
                out(slot.queue.id.get(), point.offset as u32, point.value);
                at = point.next;
            }
        }
    }
    pub fn dropped(&self) -> u32 {
        self.arena.dropped.get()
    }
}
impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}
#[allow(non_snake_case)]
impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> i32 {
        self.active.get() as i32
    }
    unsafe fn getParameterData(&self, index: i32) -> *mut IParamValueQueue {
        if index < 0 || index as usize >= self.active.get() {
            return std::ptr::null_mut();
        }
        self.queues[index as usize].interface.as_ptr()
    }
    unsafe fn addParameterData(&self, id: *const u32, index: *mut i32) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: SDK caller supplies a readable parameter-id pointer.
        let id = unsafe { id.read() };
        if id == u32::MAX {
            return std::ptr::null_mut();
        }
        let active = self.active.get();
        let at = match self
            .queues
            .iter()
            .take(active)
            .position(|slot| slot.queue.id.get() == id)
        {
            Some(at) => at,
            None if active < self.queues.len() => {
                self.queues[active].queue.id.set(id);
                self.active.set(active + 1);
                active
            }
            None => {
                self.arena.drop_point();
                return std::ptr::null_mut();
            }
        };
        // SAFETY: optional SDK output pointer.
        if !index.is_null() {
            unsafe {
                index.write(at as i32);
            }
        }
        self.queues[at].interface.as_ptr()
    }
}

pub(super) struct Events {
    data: UnsafeCell<Vec<Event>>,
    len: Cell<usize>,
    dropped: Cell<u32>,
}
impl Events {
    pub fn new() -> ComWrapper<Self> {
        // SAFETY: Event is an SDK POD struct; a zeroed unused slot is valid storage.
        let zero = unsafe { std::mem::zeroed() };
        ComWrapper::new(Self {
            data: UnsafeCell::new(vec![zero; NOTE_CAPACITY]),
            len: Cell::new(0),
            dropped: Cell::new(0),
        })
    }
    pub fn clear(&self) {
        self.len.set(0);
        self.dropped.set(0);
    }
    pub fn push(&self, mut event: Event) {
        // SAFETY: event is a live SDK POD struct, used only synchronously.
        let before = self.dropped();
        if unsafe { self.addEvent(&mut event) } != kResultOk && self.dropped() == before {
            self.dropped.set(before.saturating_add(1));
        }
    }
    pub fn dropped(&self) -> u32 {
        self.dropped.get()
    }
}
impl Class for Events {
    type Interfaces = (IEventList,);
}
#[allow(non_snake_case)]
impl IEventListTrait for Events {
    unsafe fn getEventCount(&self) -> i32 {
        self.len.get() as i32
    }
    unsafe fn getEvent(&self, index: i32, event: *mut Event) -> tresult {
        if index < 0 || index as usize >= self.len.get() || event.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: exclusively used fixed list, bounded index, writable SDK output.
        unsafe {
            event.write((&*self.data.get())[index as usize]);
        }
        kResultOk
    }
    unsafe fn addEvent(&self, event: *mut Event) -> tresult {
        if event.is_null() {
            return kInvalidArgument;
        }
        let at = self.len.get();
        if at == NOTE_CAPACITY {
            self.dropped.set(self.dropped.get().saturating_add(1));
            return kResultFalse;
        }
        // SAFETY: synchronous SDK caller supplies Event storage; fixed list is exclusive.
        let event = unsafe { event.read() };
        // Only pointer-free notes and numeric expression are retained. SysEx/text pointers do not
        // survive the plugin call and are not part of the public host API.
        if !matches!(event.r#type, 0 | 1 | 4) || event.sampleOffset < 0 {
            return kNotImplemented;
        }
        if event.r#type == 4 {
            // SAFETY: the checked discriminant selects the pointer-free value.
            let expression = unsafe { event.__field0.noteExpressionValue };
            if expression.noteId < 0
                || !expression.value.is_finite()
                || !(0.0..=1.0).contains(&expression.value)
            {
                return kInvalidArgument;
            }
        }
        // SAFETY: bounded index into fixed exclusive storage.
        unsafe {
            (&mut *self.data.get())[at] = event;
        }
        self.len.set(at + 1);
        kResultOk
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_input_sources_fit_without_coalescing_or_growing() {
        assert_eq!(INPUT_PARAMETER_CAPACITY, 16641);
        let changes = Changes::input(INPUT_PARAMETER_CAPACITY + 1);
        for id in 0..INPUT_PARAMETER_CAPACITY {
            assert!(changes.push(id as u32, id as u32, 0.5));
        }
        assert_eq!(changes.dropped(), 0);
        assert!(!changes.push(INPUT_PARAMETER_CAPACITY as u32, 0, 0.75));
        assert_eq!(changes.dropped(), 1);
        changes.clear();
        for time in 0..INPUT_PARAMETER_CAPACITY {
            assert!(changes.push(7, time as u32, 0.5));
        }
        assert!(!changes.push(7, INPUT_PARAMETER_CAPACITY as u32, 0.75));
        let mut final_value = None;
        changes.visit_final(&mut |_, value| final_value = Some(value));
        assert_eq!(final_value, Some(0.5), "refusal cannot replace readback");
        assert_eq!(changes.dropped(), 1);
    }
    #[test]
    fn parameter_queues_preserve_offsets_and_enforce_capacity() {
        let changes = Changes::new();
        changes.push(7, 3, 0.25);
        changes.push(9, 5, 0.5);
        changes.push(7, 11, 0.75);
        let mut points = Vec::new();
        changes.visit(&mut |id, time, value| points.push((id, time, value)));
        assert_eq!(points, [(7, 3, 0.25), (7, 11, 0.75), (9, 5, 0.5)]);
        changes.clear();
        for index in 0..EVENT_CAPACITY + 1 {
            changes.push(7, index as u32, 0.5);
        }
        assert_eq!(changes.dropped(), 1);
        // SAFETY: COM methods on owned bounded test buffers.
        unsafe {
            assert!(changes.getParameterData(-1).is_null());
            assert!(changes.getParameterData(1).is_null());
        }
        changes.clear();
        assert_eq!(changes.dropped(), 0);
        assert_eq!(unsafe { changes.getParameterCount() }, 0);
    }
    #[test]
    fn malformed_sdk_points_and_events_are_rejected() {
        let changes = Changes::new();
        let mut index = 0;
        // SAFETY: valid synchronous SDK arguments and owned COM buffers.
        unsafe {
            assert!(
                changes
                    .addParameterData(std::ptr::null(), &mut index)
                    .is_null()
            );
            let queue = vst3::ComRef::from_raw(changes.addParameterData(&7, &mut index)).unwrap();
            assert_eq!(queue.addPoint(-1, 0.5, &mut index), kInvalidArgument);
            assert_eq!(queue.addPoint(0, f64::NAN, &mut index), kInvalidArgument);
            assert_eq!(queue.addPoint(0, 1.5, &mut index), kInvalidArgument);
            assert_eq!(queue.addPoint(4, 0.5, &mut index), kResultOk);
            assert_eq!(queue.addPoint(4, 0.75, &mut index), kResultOk);
            assert_eq!(queue.getPointCount(), 1);
            let (mut time, mut value) = (0, 0.0);
            assert_eq!(queue.getPoint(0, &mut time, &mut value), kResultOk);
            assert_eq!((time, value), (4, 0.75));
            assert_eq!(queue.addPoint(2, 0.25, &mut index), kInvalidArgument);
            assert_eq!(
                queue.getPoint(0, std::ptr::null_mut(), std::ptr::null_mut()),
                kInvalidArgument
            );
            let events = Events::new();
            let mut event: Event = std::mem::zeroed();
            event.r#type = 2;
            assert_eq!(events.addEvent(&mut event), kNotImplemented);
            assert_eq!(events.getEvent(0, &mut event), kInvalidArgument);
        }
    }
    #[test]
    fn numeric_expressions_survive_the_sdk_list_and_refusal_counts_as_a_drop() {
        let events = Events::new();
        let mut expression: Event = unsafe { std::mem::zeroed() };
        expression.r#type = 4;
        expression.sampleOffset = 7;
        expression.__field0.noteExpressionValue = NoteExpressionValueEvent {
            typeId: 2,
            noteId: crate::FIRST_NOTE_INSTANCE_ID as i32,
            value: 0.75,
        };
        for _ in 0..crate::processor::RECOVERY_EVENT_CAPACITY {
            events.push(expression);
        }
        assert_eq!(events.dropped(), 0);
        let mut returned: Event = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { events.getEvent(0, &mut returned) }, kResultOk);
        assert_eq!(returned.r#type, 4);
        let value = unsafe { returned.__field0.noteExpressionValue };
        assert_eq!(
            (returned.sampleOffset, value.noteId, value.value),
            (7, crate::FIRST_NOTE_INSTANCE_ID as i32, 0.75)
        );
        expression.__field0.noteExpressionValue = NoteExpressionValueEvent {
            typeId: 2,
            noteId: crate::FIRST_NOTE_INSTANCE_ID as i32,
            value: f64::NAN,
        };
        events.push(expression);
        expression.r#type = 5; // Pointer-bearing text is never retained.
        events.push(expression);
        assert_eq!(events.dropped(), 2);
    }
}
