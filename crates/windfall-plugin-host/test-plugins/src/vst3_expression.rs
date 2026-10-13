//! Independent pointer-free expression probe. PCM is an algebraic identity
//! mark, not a second implementation of the host's event translation.
use super::Component;
use vst3::Steinberg::{Vst::*, *};

#[derive(Clone, Copy)]
struct Voice {
    id: i32,
    velocity: f32,
    pan: f64,
    tuning: f64,
    expression: f64,
    brightness: f64,
}
impl Voice {
    const EMPTY: Self = Self {
        id: -1,
        velocity: 0.0,
        pan: 0.5,
        tuning: 0.0,
        expression: 0.5,
        brightness: 0.5,
    };
}
pub(super) struct Voices([Voice; 1024]);
impl Default for Voices {
    fn default() -> Self {
        Self([Voice::EMPTY; 1024])
    }
}
impl Voices {
    pub(super) fn clear(&mut self) {
        self.0.fill(Voice::EMPTY);
    }
    pub(super) fn event(&mut self, event: &Event) {
        match event.r#type {
            0 => {
                let note = unsafe { event.__field0.noteOn };
                if note.noteId < 0 {
                    return;
                }
                let slot = self
                    .0
                    .iter()
                    .position(|voice| voice.id == note.noteId)
                    .or_else(|| self.0.iter().position(|voice| voice.id < 0));
                if let Some(slot) = slot {
                    self.0[slot] = Voice {
                        id: note.noteId,
                        velocity: note.velocity,
                        ..Voice::EMPTY
                    };
                }
            }
            1 => {
                let note = unsafe { event.__field0.noteOff };
                if let Some(voice) = self.0.iter_mut().find(|voice| voice.id == note.noteId) {
                    *voice = Voice::EMPTY;
                }
            }
            4 => {
                let expression = unsafe { event.__field0.noteExpressionValue };
                if let Some(voice) = self
                    .0
                    .iter_mut()
                    .find(|voice| voice.id == expression.noteId)
                {
                    match expression.typeId {
                        1 => voice.pan = expression.value,
                        2 => voice.tuning = 240.0 * (expression.value - 0.5),
                        4 => voice.expression = expression.value,
                        5 => voice.brightness = expression.value,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    pub(super) fn pcm(&self) -> [f32; 2] {
        let mut result = [0.0_f64; 2];
        for voice in &self.0 {
            if voice.id >= 0 {
                let mark = f64::from(voice.velocity)
                    * (1.0 + voice.tuning / 12.0 + voice.expression / 4.0 + voice.brightness / 8.0);
                result[0] += mark * (1.0 - voice.pan);
                result[1] += mark * voice.pan;
            }
        }
        result.map(|value| value as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reused_identity_updates_existing_owner_even_with_an_earlier_empty_slot() {
        let mut voices = Voices::default();
        voices.0[1] = Voice {
            id: 2048,
            velocity: 0.75,
            ..Voice::EMPTY
        };
        let mut event: Event = unsafe { std::mem::zeroed() };
        event.r#type = 0;
        event.__field0.noteOn = NoteOnEvent {
            channel: 2,
            pitch: 69,
            tuning: 0.0,
            velocity: 0.5,
            length: 0,
            noteId: 2048,
        };
        voices.event(&event);
        assert_eq!(voices.0.iter().filter(|voice| voice.id == 2048).count(), 1);
        assert_eq!(voices.0[1].velocity, 0.5);
    }
}

#[allow(non_snake_case)]
impl INoteExpressionControllerTrait for Component {
    unsafe fn getNoteExpressionCount(&self, bus: i32, channel: i16) -> i32 {
        // Channel 16 deliberately has no expression: negotiation must be per channel.
        if (self.expression_probe
            || self.dynamic && self.dynamic_mode.load(std::sync::atomic::Ordering::Relaxed) == 5)
            && bus == 0
            && (0..15).contains(&channel)
        {
            4
        } else {
            0
        }
    }
    unsafe fn getNoteExpressionInfo(
        &self,
        bus: i32,
        channel: i16,
        index: i32,
        info: *mut NoteExpressionTypeInfo,
    ) -> tresult {
        if info.is_null()
            || index < 0
            || index >= unsafe { self.getNoteExpressionCount(bus, channel) }
        {
            return kInvalidArgument;
        }
        let mut value: NoteExpressionTypeInfo = unsafe { std::mem::zeroed() };
        value.typeId = [1, 2, 4, 5][index as usize];
        value.unitId = -1;
        value.valueDesc.defaultValue = 0.5;
        value.valueDesc.maximum = 1.0;
        unsafe {
            info.write(value);
        }
        kResultOk
    }
    unsafe fn getNoteExpressionStringByValue(
        &self,
        _bus: i32,
        _channel: i16,
        _id: u32,
        _value: f64,
        _text: *mut String128,
    ) -> tresult {
        kNotImplemented
    }
    unsafe fn getNoteExpressionValueByString(
        &self,
        _bus: i32,
        _channel: i16,
        _id: u32,
        _text: *const TChar,
        _value: *mut f64,
    ) -> tresult {
        kNotImplemented
    }
}
