//! Immutable data an effect needs that is too large or too free-form for a
//! `Copy` parameter struct: drawn curves and loaded audio.
//!
//! # The contract
//!
//! - A parameter struct holds a [`CurveRef`], a [`ShapeRef`] or a source
//!   key: eight bytes or fewer, `Copy`, comparable. The data itself lives
//!   here and never changes once published.
//! - **Publishing** happens away from the audio thread: when a command or a
//!   project file is deserialized, or when the engine compiles a plan. It
//!   allocates and takes a lock that only other publishers contend for.
//! - **Reading** is for any thread, the audio thread included. A lookup
//!   reads atomics and walks a bounded number of slots. It never locks,
//!   allocates, frees or waits. It returns a lease whose final owner remains
//!   in the store until control-side collection. Processors cache leases.
//! - **Persistence.** A curve is saved by content: its reference serializes
//!   as the list of points and deserializes by publishing them. Its id is a
//!   hash of the content, so equal curves are one asset and a saved project
//!   needs no id table. A source is saved as the project's sample id; the
//!   engine publishes the decoded audio under that id before it builds the
//!   effect.
//! - **Bounds.** The store refuses to grow past
//!   [`MAX_CURVES`] curves, [`CURVE_BYTE_BUDGET`] bytes of curve data,
//!   [`MAX_SOURCES`] source entries or [`SOURCE_BYTE_BUDGET`] bytes of
//!   source audio. A refused curve is an error the command reports; a
//!   refused source leaves the effect without audio. [`stats`] counts both.
//!
//! Documents pin the current project and both sides of retained edits; plans
//! pin their parameters independently. Releasing history or replacing a
//! project releases those roots. The Session retirement pump withdraws and
//! frees unused managed entries after readers finish acquiring their leases.
//! Fresh standalone publications must be adopted with AssetPins before they
//! participate in collection. Copy references alone are not ownership.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;
use windfall_core::AudioBuffer;

/// Most points one curve can hold.
pub const MAX_CURVE_POINTS: usize = 64;
/// Most distinct curves the process can hold.
pub const MAX_CURVES: usize = 1 << 15;
/// Most bytes of curve points and tables the process can hold.
pub const CURVE_BYTE_BUDGET: usize = 64 * 1024 * 1024;
/// Entries in a shaping table, spanning inputs 0 to 1 inclusive.
pub const SHAPE_TABLE_POINTS: usize = 2_049;
/// Most source entries the process can hold. A sample id whose audio is
/// replaced takes a new entry.
pub const MAX_SOURCES: usize = 1_024;
/// Most bytes of source audio the store will keep alive.
pub const SOURCE_BYTE_BUDGET: usize = 1 << 30;

const CURVE_SLOTS: usize = MAX_CURVES * 2;

/// One breakpoint of a drawn curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CurvePoint {
    /// Position along the curve, 0 to 1.
    pub x: f32,
    /// Value there, 0 to 1. What it stands for is up to the effect.
    pub y: f32,
    /// Bend of the segment that starts here, -1 to 1. Zero is a straight
    /// line, positive values start slowly and negative ones start fast.
    #[serde(default)]
    pub tension: f32,
}

impl CurvePoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y, tension: 0.0 }
    }
}

/// Why a curve or a source was not published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetError {
    /// More than [`MAX_CURVE_POINTS`] points.
    TooManyPoints,
    /// The store is at one of its bounds.
    Full,
    /// Two different curves hash to one id. Not expected in practice.
    Collision,
}

impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyPoints => {
                write!(f, "a curve holds at most {MAX_CURVE_POINTS} points")
            }
            Self::Full => f.write_str("the effect asset memory is full; release unused project history or sources"),
            Self::Collision => f.write_str("the curve could not be stored; move a point slightly"),
        }
    }
}

impl std::error::Error for AssetError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurveKind {
    /// Points only, evaluated exactly.
    Curve,
    /// Points and a table rendered from them.
    Shape,
}

/// A published curve.
#[derive(Debug)]
pub struct CurveAsset {
    id: u64,
    kind: CurveKind,
    points: Box<[CurvePoint]>,
    table: Option<Box<[f32]>>,
    managed: AtomicBool,
}

fn bend(t: f32, tension: f32) -> f32 {
    if tension == 0.0 {
        t
    } else {
        // 4^tension: a quarter power at -1, a fourth power at 1.
        t.powf((tension * std::f32::consts::LN_2 * 2.0).exp())
    }
}

/// The value of sorted, clean `points` at `x`. Left of the first point is
/// its value and right of the last is its value. Where several points share
/// an `x` the last one counts from there on, which is how a curve jumps.
fn value_of(points: &[CurvePoint], x: f32) -> f32 {
    let after = points.partition_point(|point| point.x <= x);
    if after == 0 {
        return points.first().map_or(0.0, |point| point.y);
    }
    let from = points[after - 1];
    let Some(to) = points.get(after) else {
        return from.y;
    };
    let span = to.x - from.x;
    if span <= 0.0 {
        return from.y;
    }
    from.y + (to.y - from.y) * bend((x - from.x) / span, from.tension)
}

impl CurveAsset {
    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    /// The curve's value at `x`, exactly as drawn. See [`CurvePoint`].
    #[inline]
    pub fn value(&self, x: f32) -> f32 {
        value_of(&self.points, x)
    }

    /// The table of a shape: [`SHAPE_TABLE_POINTS`] values at evenly spaced
    /// `x` from 0 to 1. `None` for a plain curve.
    pub fn table(&self) -> Option<&[f32]> {
        self.table.as_deref()
    }

    /// Reads the table between entries. Falls back to the exact value for a
    /// plain curve.
    #[inline]
    pub fn shaped(&self, x: f32) -> f32 {
        let Some(table) = &self.table else {
            return self.value(x);
        };
        let position = x.clamp(0.0, 1.0) * (SHAPE_TABLE_POINTS - 1) as f32;
        let index = (position as usize).min(SHAPE_TABLE_POINTS - 2);
        let from = table[index];
        from + (table[index + 1] - from) * (position - index as f32)
    }

    fn bytes(points: usize, kind: CurveKind) -> usize {
        std::mem::size_of::<Self>()
            + points * std::mem::size_of::<CurvePoint>()
            + match kind {
                CurveKind::Curve => 0,
                CurveKind::Shape => SHAPE_TABLE_POINTS * std::mem::size_of::<f32>(),
            }
    }
}

/// Brings points into range, replaces anything that is not a number and
/// sorts by `x`, keeping the order of points that share one.
fn canonical(points: &[CurvePoint]) -> Result<Vec<CurvePoint>, AssetError> {
    if points.len() > MAX_CURVE_POINTS {
        return Err(AssetError::TooManyPoints);
    }
    let clean = |value: f32, min: f32| {
        if value.is_finite() {
            // Adding zero turns -0.0 into 0.0, so equal curves hash alike.
            value.clamp(min, 1.0) + 0.0
        } else {
            0.0
        }
    };
    let mut points: Vec<CurvePoint> = points
        .iter()
        .map(|point| CurvePoint {
            x: clean(point.x, 0.0),
            y: clean(point.y, 0.0),
            tension: clean(point.tension, -1.0),
        })
        .collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    Ok(points)
}

/// FNV-1a over the kind and the bit patterns of the points. Never zero.
fn hash(kind: CurveKind, points: &[CurvePoint]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut feed = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    feed(match kind {
        CurveKind::Curve => 1,
        CurveKind::Shape => 2,
    });
    for point in points {
        for value in [point.x, point.y, point.tension] {
            for byte in value.to_bits().to_le_bytes() {
                feed(byte);
            }
        }
    }
    hash.max(1)
}

#[derive(Default)]
struct Store {
    curves: Vec<(usize, Arc<CurveAsset>)>,
    sources: Vec<(usize, Arc<SourceEntry>)>,
    retired_curves: Vec<Arc<CurveAsset>>,
    retired_sources: Vec<Arc<SourceEntry>>,
    curve_bytes: usize,
    source_bytes: usize,
}

/// Serializes publishers/collectors. Readers never take it.
static WRITER: Mutex<Store> = Mutex::new(Store {
    curves: Vec::new(), sources: Vec::new(),
    retired_curves: Vec::new(), retired_sources: Vec::new(),
    curve_bytes: 0, source_bytes: 0,
});
static REFUSED: AtomicU64 = AtomicU64::new(0);
static SOURCE_EPOCH: AtomicU64 = AtomicU64::new(0);
// Protects the gap between loading a published pointer and acquiring its Arc.
// Sequential consistency pairs pointer withdrawal with the collector's check.
static READERS: AtomicUsize = AtomicUsize::new(0);

struct Slot<T> {
    id: AtomicU64,
    pointer: AtomicPtr<T>,
}
impl<T> Slot<T> {
    fn new() -> Self {
        Self { id: AtomicU64::new(0), pointer: AtomicPtr::new(std::ptr::null_mut()) }
    }
    fn read(&self) -> Option<Arc<T>> {
        READERS.fetch_add(1, Ordering::SeqCst);
        let pointer = self.pointer.load(Ordering::SeqCst);
        let value = if pointer.is_null() {
            None
        } else {
            // SAFETY: WRITER owns an Arc for every published pointer. Removal
            // retains that Arc in the retirement list until READERS == 0.
            // The acquired Arc is likewise backed by a store owner until its
            // last lease is released, so dropping it on audio never frees.
            unsafe {
                Arc::increment_strong_count(pointer);
                Some(Arc::from_raw(pointer))
            }
        };
        READERS.fetch_sub(1, Ordering::SeqCst);
        value
    }
    fn publish(&self, id: u64, value: &Arc<T>) {
        self.id.store(id, Ordering::Release);
        self.pointer.store(Arc::as_ptr(value).cast_mut(), Ordering::SeqCst);
    }
}

type CurveSlots = Box<[Slot<CurveAsset>]>;
static CURVES: OnceLock<CurveSlots> = OnceLock::new();

fn curve_slots() -> &'static CurveSlots {
    CURVES.get_or_init(|| (0..CURVE_SLOTS).map(|_| Slot::new()).collect())
}

fn publish_curve(kind: CurveKind, points: &[CurvePoint]) -> Result<u64, AssetError> {
    let points = canonical(points)?;
    if points.is_empty() {
        return Ok(0);
    }
    let id = hash(kind, &points);
    let slots = curve_slots();
    let mut store = WRITER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, found)) = store.curves.iter().find(|(_, asset)| asset.id == id) {
        return if found.kind == kind && *found.points == *points {
            Ok(id)
        } else {
            Err(AssetError::Collision)
        };
    }
    let bytes = CurveAsset::bytes(points.len(), kind);
    if store.curves.len() + store.retired_curves.len() >= MAX_CURVES
        || store.curve_bytes + bytes > CURVE_BYTE_BUDGET {
        REFUSED.fetch_add(1, Ordering::Relaxed);
        return Err(AssetError::Full);
    }
    let mask = slots.len() - 1;
    let mut index = (id as usize) & mask;
    for _ in 0..slots.len() {
        if slots[index].pointer.load(Ordering::SeqCst).is_null() {
            let table = match kind {
                CurveKind::Curve => None,
                CurveKind::Shape => Some((0..SHAPE_TABLE_POINTS)
                    .map(|n| value_of(&points, n as f32 / (SHAPE_TABLE_POINTS - 1) as f32))
                    .collect()),
            };
            let asset = Arc::new(CurveAsset {
                id, kind, points: points.into_boxed_slice(), table,
                managed: AtomicBool::new(false),
            });
            slots[index].publish(id, &asset);
            store.curves.push((index, asset));
            store.curve_bytes += bytes;
            return Ok(id);
        }
        index = (index + 1) & mask;
    }
    REFUSED.fetch_add(1, Ordering::Relaxed);
    Err(AssetError::Full)
}

/// Finds a published curve. Safe on the audio thread.
fn find_curve(id: u64) -> Option<Arc<CurveAsset>> {
    if id == 0 {
        return None;
    }
    let slots: &'static CurveSlots = CURVES.get()?;
    let mask = slots.len() - 1;
    let mut index = (id as usize) & mask;
    for _ in 0..slots.len() {
        // Reclaimed slots are tombstones. Never stop at a withdrawn pointer.
        let stored = slots[index].id.load(Ordering::Acquire);
        if stored == 0 { return None; }
        if stored == id {
            if let Some(found) = slots[index].read() {
                if found.id == id { return Some(found); }
            }
        }
        index = (index + 1) & mask;
    }
    None
}

macro_rules! curve_reference {
    ($(#[$meta:meta])* $name:ident, $kind:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct $name(u64);

        impl $name {
            /// No curve. Serializes as an empty list.
            pub const NONE: Self = Self(0);

            /// Publishes `points` and returns the reference to them. Equal
            /// points give equal references. An empty list is [`Self::NONE`].
            /// Allocates and locks: not for the audio thread.
            pub fn new(points: &[CurvePoint]) -> Result<Self, AssetError> {
                publish_curve($kind, points).map(Self)
            }

            pub fn is_none(self) -> bool {
                self.0 == 0
            }

            /// The published curve. Safe on the audio thread.
            #[inline]
            pub fn asset(self) -> Option<Arc<CurveAsset>> {
                find_curve(self.0)
            }

            /// Copies the points for control-side inspection. Allocates;
            /// callbacks should cache `asset()` and borrow its points instead.
            pub fn points(self) -> Vec<CurvePoint> {
                self.asset().map_or_else(Vec::new, |asset| asset.points().to_vec())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self.asset() {
                    Some(asset) => asset.points().serialize(serializer),
                    None if self.is_none() => <[CurvePoint]>::serialize(&[], serializer),
                    None => Err(<S::Error as serde::ser::Error>::custom(
                        "the effect curve is no longer available; take a new snapshot before saving")),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let points = deserialize_points(deserializer)?;
                Self::new(&points).map_err(D::Error::custom)
            }
        }
    };
}

/// Bound temporary deserialization storage too, before publishing an asset.
fn deserialize_points<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<CurvePoint>, D::Error> {
    struct Points;
    impl<'de> serde::de::Visitor<'de> for Points {
        type Value = Vec<CurvePoint>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "at most {MAX_CURVE_POINTS} effect curve points")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> Result<Self::Value, A::Error> {
            if sequence.size_hint().is_some_and(|n| n > MAX_CURVE_POINTS) {
                return Err(A::Error::custom(AssetError::TooManyPoints));
            }
            let mut points = Vec::with_capacity(MAX_CURVE_POINTS);
            while let Some(point) = sequence.next_element()? {
                if points.len() == MAX_CURVE_POINTS {
                    return Err(A::Error::custom(AssetError::TooManyPoints));
                }
                points.push(point);
            }
            Ok(points)
        }
    }
    deserializer.deserialize_seq(Points)
}

curve_reference!(
    /// A reference to a drawn curve that is evaluated exactly, such as an
    /// envelope over a bar. Declare its TypeScript type on the field with
    /// `#[ts(as = "Vec<CurvePoint>")]`.
    CurveRef,
    CurveKind::Curve
);
curve_reference!(
    /// A reference to a drawn curve that also carries a rendered table,
    /// for per-sample lookup in a waveshaper.
    ShapeRef,
    CurveKind::Shape
);

struct SourceEntry {
    id: u64,
    key: u32,
    audio: AudioBuffer,
    managed: AtomicBool,
}

/// A source lease. Its final owner stays in the control-side store.
#[derive(Clone)]
pub struct AudioLease(Arc<SourceEntry>);
impl std::fmt::Debug for AudioLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AudioLease").field(&self.0.id).finish()
    }
}
impl std::ops::Deref for AudioLease {
    type Target = AudioBuffer;
    fn deref(&self) -> &Self::Target { &self.0.audio }
}

/// A runtime source allocation, independent of project-local sample numbers.
/// Plans publish it off-thread; callbacks only retain this small index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceRef(u64);
impl SourceRef {
    pub const NONE: Self = Self(0);
    pub fn audio(self) -> Option<AudioLease> {
        if self.0 == 0 { return None; }
        for slot in SOURCES.get()?.iter() {
            if slot.id.load(Ordering::Acquire) == self.0 {
                if let Some(entry) = slot.read() {
                    if entry.id == self.0 { return Some(AudioLease(entry)); }
                }
            }
        }
        None
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// Publish by allocation identity, so two projects with SampleId(1) cannot
/// change each other's prepared scratch sources. No PCM copy is made.
pub fn publish_audio(audio: &AudioBuffer) -> Result<SourceRef, AssetError> {
    publish_keyed_audio(0, audio)
}

type SourceSlots = Box<[Slot<SourceEntry>]>;
static SOURCES: OnceLock<SourceSlots> = OnceLock::new();

fn source_slots() -> &'static SourceSlots {
    SOURCES.get_or_init(|| (0..MAX_SOURCES).map(|_| Slot::new()).collect())
}

/// Makes `audio` the source for `key`, which is a project's sample id.
/// Publishing the audio a key already has does nothing. Publishing other
/// audio under a known key replaces it for every later lookup. Allocates
/// and locks: not for the audio thread.
pub fn publish_source(key: u32, audio: &AudioBuffer) -> Result<(), AssetError> {
    publish_keyed_audio(key, audio).map(|_| ())
}

fn publish_keyed_audio(key: u32, audio: &AudioBuffer) -> Result<SourceRef, AssetError> {
    let slots = source_slots();
    let mut store = WRITER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, entry)) = store.sources.iter().filter(|(_, entry)|
        entry.key == key && (key != 0 || entry.audio.identity() == audio.identity()))
        .max_by_key(|(_, entry)| entry.id) {
        if entry.audio.identity() == audio.identity() { return Ok(SourceRef(entry.id)); }
    }
    let bytes = std::mem::size_of_val(audio.samples());
    let free = slots.iter().position(|slot| slot.pointer.load(Ordering::SeqCst).is_null());
    let Some(free) = free.filter(|_| store.source_bytes + bytes <= SOURCE_BYTE_BUDGET
        && store.sources.len() + store.retired_sources.len() < MAX_SOURCES) else {
        REFUSED.fetch_add(1, Ordering::Relaxed);
        return Err(AssetError::Full);
    };
    let id = SOURCE_EPOCH.fetch_update(Ordering::AcqRel, Ordering::Acquire,
        |id| id.checked_add(1)).map_err(|_| AssetError::Full)? + 1;
    let entry = Arc::new(SourceEntry {
        id, key, audio: audio.clone(), managed: AtomicBool::new(false),
    });
    slots[free].publish(id, &entry);
    store.sources.push((free, entry));
    store.source_bytes += bytes;
    Ok(SourceRef(id))
}

/// The audio last published for `key`. Safe on the audio thread: it walks
/// at most [`MAX_SOURCES`] slots.
pub fn source(key: u32) -> Option<AudioLease> {
    let slots: &'static SourceSlots = SOURCES.get()?;
    let mut latest = None;
    for slot in slots.iter() {
        let entry = match slot.read() {
            Some(entry) => entry,
            None => continue,
        };
        if entry.key == key && latest.as_ref().is_none_or(|known: &AudioLease| known.0.id < entry.id) {
            latest = Some(AudioLease(entry));
        }
    }
    latest
}

/// Changes whenever a source is published. A processor that remembers the
/// value it last saw knows when to look its source up again.
#[inline]
pub fn source_epoch() -> u64 {
    SOURCE_EPOCH.load(Ordering::Acquire)
}

/// Control-side ownership for Copy effect parameters. Keep this with the
/// document/history/immutable plan that can restore those parameters. A plain
/// freshly published reference remains unmanaged until an owner adopts it.
#[derive(Debug, Clone, Default)]
pub struct AssetPins {
    curves: Vec<Arc<CurveAsset>>,
    sources: Vec<AudioLease>,
}
impl AssetPins {
    pub fn for_effects<'a>(effects: impl IntoIterator<Item = &'a crate::EffectParams>) -> Self {
        let mut pins = Self::default();
        for effect in effects {
            let (curves, source) = match effect {
                crate::EffectParams::Waveshaper(p) => ([p.shape.asset(), None], None),
                crate::EffectParams::VolumeGate(p) =>
                    ([p.time_curve.asset(), p.volume_curve.asset()], None),
                crate::EffectParams::Scratch(p) =>
                    ([p.position_curve.asset(), None], p.runtime_source.audio()),
                _ => ([None, None], None),
            };
            for curve in curves.into_iter().flatten() {
                curve.managed.store(true, Ordering::Release);
                if !pins.curves.iter().any(|known| Arc::ptr_eq(known, &curve)) {
                    pins.curves.push(curve);
                }
            }
            if let Some(source) = source {
                source.0.managed.store(true, Ordering::Release);
                if !pins.sources.iter().any(|known| Arc::ptr_eq(&known.0, &source.0)) {
                    pins.sources.push(source);
                }
            }
        }
        pins
    }
    pub fn extend(&mut self, other: Self) {
        for curve in other.curves {
            if !self.curves.iter().any(|known| Arc::ptr_eq(known, &curve)) {
                self.curves.push(curve);
            }
        }
        for source in other.sources {
            if !self.sources.iter().any(|known| Arc::ptr_eq(&known.0, &source.0)) {
                self.sources.push(source);
            }
        }
    }
}

/// Withdraw unused managed assets and free retired allocations on the caller's
/// control thread. Never call from audio. Live readers and crossfade leases
/// retain a store owner, including after withdrawal; they cannot free it.
pub fn collect_retired() {
    let mut store = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut index = 0;
    while index < store.curves.len() {
        let (slot, value) = &store.curves[index];
        if value.managed.load(Ordering::Acquire) && Arc::strong_count(value) == 1 {
            curve_slots()[*slot].pointer.store(std::ptr::null_mut(), Ordering::SeqCst);
            let (_, value) = store.curves.swap_remove(index);
            store.retired_curves.push(value);
        } else { index += 1; }
    }
    let mut index = 0;
    while index < store.sources.len() {
        let (slot, value) = &store.sources[index];
        if value.managed.load(Ordering::Acquire) && Arc::strong_count(value) == 1 {
            source_slots()[*slot].pointer.store(std::ptr::null_mut(), Ordering::SeqCst);
            let (_, value) = store.sources.swap_remove(index);
            store.retired_sources.push(value);
        } else { index += 1; }
    }
    if READERS.load(Ordering::SeqCst) != 0 { return; }
    let mut index = 0;
    while index < store.retired_curves.len() {
        if Arc::strong_count(&store.retired_curves[index]) == 1 {
            let asset = store.retired_curves.swap_remove(index);
            store.curve_bytes -= CurveAsset::bytes(asset.points.len(), asset.kind);
            drop(asset);
        } else { index += 1; }
    }
    let mut index = 0;
    while index < store.retired_sources.len() {
        if Arc::strong_count(&store.retired_sources[index]) == 1 {
            let entry = store.retired_sources.swap_remove(index);
            store.source_bytes -= std::mem::size_of_val(entry.audio.samples());
            drop(entry);
        } else { index += 1; }
    }
}

/// How full the store is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetStats {
    pub curves: usize,
    pub curve_bytes: usize,
    pub sources: usize,
    pub source_bytes: usize,
    /// Publications refused at a bound since the process started.
    pub refused: u64,
}

/// Locks, so not for the audio thread.
pub fn stats() -> AssetStats {
    let store = WRITER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    AssetStats {
        curves: store.curves.len() + store.retired_curves.len(),
        curve_bytes: store.curve_bytes,
        sources: store.sources.len() + store.retired_sources.len(),
        source_bytes: store.source_bytes,
        refused: REFUSED.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> CurvePoint {
        CurvePoint::new(x, y)
    }

    #[test]
    fn managed_sources_reuse_capacity_without_redirecting_reclaimed_handles() {
        use crate::performance::ScratchParams;
        let mut first = SourceRef::NONE;
        for n in 0..MAX_SOURCES + 8 {
            let audio = AudioBuffer::from_interleaved(48_000, 1, vec![n as f32; 8]);
            let source = publish_audio(&audio).unwrap();
            if n == 0 { first = source; }
            let params = crate::EffectParams::Scratch(ScratchParams {
                runtime_source: source, ..Default::default()
            });
            let pins = AssetPins::for_effects([&params]);
            let lease = source.audio().unwrap();
            collect_retired();
            assert_eq!(lease.samples()[0], n as f32);
            drop(pins);
            collect_retired();
            assert!(source.audio().is_some(), "a reader is still an owner");
            drop(lease);
            collect_retired();
            assert!(source.audio().is_none());
            assert!(first.audio().is_none(), "reused slots must not reuse source identities");
        }
    }

    #[test]
    fn curve_leases_remain_valid_during_control_side_collection() {
        let curve = CurveRef::new(&[point(0.0, 0.014719), point(1.0, 0.985219)]).unwrap();
        let params = crate::EffectParams::VolumeGate(crate::performance::VolumeGateParams {
            time_curve: curve, ..Default::default()
        });
        let pins = AssetPins::for_effects([&params]);
        let weak = Arc::downgrade(&curve.asset().unwrap());
        let lease = curve.asset().unwrap();
        drop(pins);
        let worker = std::thread::spawn(|| { for _ in 0..200 { collect_retired(); } });
        for _ in 0..20_000 { assert_eq!(lease.value(0.0), 0.014719); }
        worker.join().unwrap();
        assert!(weak.upgrade().is_some());
        drop(lease);
        collect_retired();
        assert!(weak.upgrade().is_none());
        assert!(curve.asset().is_none());
        assert!(serde_json::to_string(&curve).unwrap_err().to_string().contains("no longer available"));
    }

    #[test]
    fn deserialization_rejects_more_than_sixty_four_points() {
        let points = vec![CurvePoint::new(0.0, 0.5); MAX_CURVE_POINTS + 1];
        let encoded = serde_json::to_string(&points).unwrap();
        assert!(
            serde_json::from_str::<CurveRef>(&encoded)
                .unwrap_err()
                .to_string()
                .contains("at most 64")
        );
        assert!(serde_json::from_str::<ShapeRef>(&encoded).is_err());
        assert!(
            serde_json::from_str::<CurveRef>(
                &serde_json::to_string(&points[..MAX_CURVE_POINTS]).unwrap()
            )
            .is_ok()
        );
    }

    #[test]
    fn equal_points_are_one_asset_and_different_points_are_not() {
        let a = CurveRef::new(&[point(0.0, 0.125), point(1.0, 0.875)]).unwrap();
        let b = CurveRef::new(&[point(0.0, 0.125), point(1.0, 0.875)]).unwrap();
        let c = CurveRef::new(&[point(0.0, 0.125), point(1.0, 0.75)]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(Arc::ptr_eq(&a.asset().unwrap(), &b.asset().unwrap()));
        assert!(!a.is_none());
        assert_eq!(CurveRef::new(&[]).unwrap(), CurveRef::NONE);
        assert!(CurveRef::NONE.asset().is_none());
        assert!(CurveRef::NONE.points().is_empty());
        // A shape of the same points is another asset: it carries a table.
        let shape = ShapeRef::new(&[point(0.0, 0.125), point(1.0, 0.875)]).unwrap();
        assert!(shape.asset().unwrap().table().is_some());
        assert!(a.asset().unwrap().table().is_none());
    }

    #[test]
    fn points_are_cleaned_and_sorted_before_they_are_stored() {
        let curve = CurveRef::new(&[
            CurvePoint {
                x: 0.75,
                y: 4.0,
                tension: -9.0,
            },
            CurvePoint {
                x: f32::NAN,
                y: -0.0,
                tension: f32::INFINITY,
            },
            point(0.25, 0.5),
        ])
        .unwrap();
        assert_eq!(
            curve.points(),
            [
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.25, 0.5),
                CurvePoint {
                    x: 0.75,
                    y: 1.0,
                    tension: -1.0
                },
            ]
        );
        let many = vec![point(0.5, 0.5); MAX_CURVE_POINTS + 1];
        assert_eq!(CurveRef::new(&many), Err(AssetError::TooManyPoints));
        assert!(CurveRef::new(&many[..MAX_CURVE_POINTS]).is_ok());
    }

    #[test]
    fn a_curve_interpolates_holds_its_ends_and_jumps_where_points_share_an_x() {
        let curve = CurveRef::new(&[
            point(0.25, 0.0),
            point(0.5, 1.0),
            point(0.5, 0.25),
            point(0.75, 0.75),
        ])
        .unwrap();
        let asset = curve.asset().unwrap();
        assert_eq!(asset.value(0.0), 0.0);
        assert_eq!(asset.value(0.25), 0.0);
        assert_eq!(asset.value(0.375), 0.5);
        // Just left of the jump is the first point's side, the jump itself
        // and everything after belong to the second.
        assert!(asset.value(0.499_99) > 0.99);
        assert_eq!(asset.value(0.5), 0.25);
        assert_eq!(asset.value(0.625), 0.5);
        assert_eq!(asset.value(0.75), 0.75);
        assert_eq!(asset.value(1.0), 0.75);
    }

    #[test]
    fn tension_bends_a_segment_without_moving_its_ends() {
        let bent = |tension| {
            CurveRef::new(&[
                CurvePoint {
                    x: 0.0,
                    y: 0.0,
                    tension,
                },
                point(1.0, 1.0),
            ])
            .unwrap()
            .asset()
            .unwrap()
        };
        assert_eq!(bent(0.0).value(0.5), 0.5);
        assert!((bent(1.0).value(0.5) - 0.0625).abs() < 1.0e-6);
        assert!((bent(-1.0).value(0.5) - 0.5_f32.powf(0.25)).abs() < 1.0e-6);
        for tension in [-1.0, -0.3, 0.6, 1.0] {
            assert_eq!(bent(tension).value(0.0), 0.0);
            assert_eq!(bent(tension).value(1.0), 1.0);
        }
    }

    #[test]
    fn a_shape_table_follows_the_curve() {
        let shape = ShapeRef::new(&[point(0.0, 0.0), point(0.5, 1.0), point(1.0, 0.5)]).unwrap();
        let asset = shape.asset().unwrap();
        assert_eq!(asset.table().unwrap().len(), SHAPE_TABLE_POINTS);
        for n in 0..=200 {
            let x = n as f32 / 200.0;
            assert!((asset.shaped(x) - asset.value(x)).abs() < 1.0e-3, "at {x}");
        }
        assert_eq!(asset.shaped(-3.0), asset.value(0.0));
        assert_eq!(asset.shaped(3.0), asset.value(1.0));
    }

    #[test]
    fn a_reference_is_saved_as_its_points_and_loads_back_equal() {
        let curve = CurveRef::new(&[
            point(0.0, 1.0),
            CurvePoint {
                x: 0.5,
                y: 0.25,
                tension: 0.5,
            },
        ])
        .unwrap();
        let json = serde_json::to_value(curve).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                { "x": 0.0, "y": 1.0, "tension": 0.0 },
                { "x": 0.5, "y": 0.25, "tension": 0.5 },
            ])
        );
        assert_eq!(serde_json::from_value::<CurveRef>(json).unwrap(), curve);
        // Tension may be left out, and an empty list is no curve.
        let bare: CurveRef = serde_json::from_str(r#"[{"x":0,"y":1},{"x":0.5,"y":0.25}]"#).unwrap();
        assert_eq!(bare.points()[1].tension, 0.0);
        assert_eq!(serde_json::to_string(&CurveRef::NONE).unwrap(), "[]");
        assert_eq!(
            serde_json::from_str::<CurveRef>("[]").unwrap(),
            CurveRef::NONE
        );
        let too_many = serde_json::to_string(&vec![point(0.5, 0.5); MAX_CURVE_POINTS + 1]).unwrap();
        let error = serde_json::from_str::<CurveRef>(&too_many).unwrap_err();
        assert!(error.to_string().contains("at most 64 points"), "{error}");
    }

    #[test]
    fn a_source_is_found_by_key_and_the_latest_audio_wins() {
        // Keys no other test uses: the store is shared by the whole process.
        let key = 0xF00D_0001;
        assert!(source(key).is_none());
        let first = AudioBuffer::from_interleaved(48_000, 2, vec![0.25; 64]);
        let before = source_epoch();
        publish_source(key, &first).unwrap();
        assert!(source_epoch() > before);
        assert_eq!(source(key).unwrap().identity(), first.identity());
        // The same audio again takes no new entry.
        let sources = stats().sources;
        let epoch = source_epoch();
        publish_source(key, &first).unwrap();
        assert_eq!(source_epoch(), epoch);
        assert!(stats().sources >= sources);
        let second = AudioBuffer::from_interleaved(44_100, 1, vec![0.5; 32]);
        publish_source(key, &second).unwrap();
        assert_eq!(source(key).unwrap().identity(), second.identity());
        assert_eq!(source(key).unwrap().sample_rate(), 44_100);
        assert!(source(0xF00D_0002).is_none());
    }

    #[test]
    fn readers_see_whole_assets_while_another_thread_publishes() {
        let writer = std::thread::spawn(|| {
            (0..400)
                .map(|n| {
                    CurveRef::new(&[point(0.0, n as f32 / 1_000.0), point(1.0, 0.123)]).unwrap()
                })
                .collect::<Vec<_>>()
        });
        let known = CurveRef::new(&[point(0.0, 0.5), point(1.0, 0.123)]).unwrap();
        for _ in 0..20_000 {
            let asset = known.asset().expect("a published curve stays findable");
            assert_eq!(asset.points().len(), 2);
            assert_eq!(asset.value(0.0), 0.5);
        }
        for (n, curve) in writer.join().unwrap().into_iter().enumerate() {
            assert_eq!(curve.asset().unwrap().value(0.0), n as f32 / 1_000.0);
        }
    }
}
