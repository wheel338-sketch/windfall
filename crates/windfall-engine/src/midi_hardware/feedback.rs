//! Note-driven controller feedback through the existing MIDI output worker.
//! This is a virtual output adapter: it sends no MIDI to physical instruments.
use super::{MidiMessage, Output};
use crate::peripherals::{NoteHaptics, PeripheralAdapter, PeripheralCommand};

pub const CONTROLLER_FEEDBACK_OUTPUT: &str = "windfall:xinput-feedback";

/// Owned by the port worker. MIDI output channels 1..4 select XInput controllers
/// 0..3; keys 0..63 select the low motor and 64..127 the high motor.
pub struct FeedbackOutput<A: PeripheralAdapter> {
    adapter: A,
    notes: [NoteHaptics; 4],
}
impl<A: PeripheralAdapter> FeedbackOutput<A> {
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            notes: std::array::from_fn(|index| {
                NoteHaptics::new(index as u8).expect("four controller indexes")
            }),
        }
    }
}
impl<A: PeripheralAdapter> Output for FeedbackOutput<A> {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        let message = MidiMessage::parse(bytes).ok_or("invalid feedback note message")?;
        // Panic on a standard worker disconnect emits every MIDI channel, so
        // unused channels must be harmless. They never address another device.
        if message.channel().is_some_and(|channel| channel >= 4) {
            return Ok(());
        }
        let command = match message {
            MidiMessage::NoteOn {
                channel,
                key,
                velocity,
            } => Some(
                self.notes[channel as usize]
                    .note(0, key, f32::from(velocity) / 127.0)
                    .command,
            ),
            MidiMessage::NoteOff { channel, key } => {
                Some(self.notes[channel as usize].note(0, key, 0.0).command)
            }
            MidiMessage::Controller {
                channel,
                number: 120 | 123,
                ..
            } => {
                self.notes[channel as usize] =
                    NoteHaptics::new(channel).expect("validated controller channel");
                Some(PeripheralCommand::Rumble {
                    controller: channel,
                    low: 0,
                    high: 0,
                })
            }
            MidiMessage::Stop => {
                for (index, notes) in self.notes.iter_mut().enumerate() {
                    *notes = NoteHaptics::new(index as u8).expect("controller index");
                }
                Some(PeripheralCommand::Stop)
            }
            // Bank/program/pitch/clock messages are valid initialization output,
            // but have no motor meaning. Velocity automation is note strength.
            _ => None,
        };
        if let Some(command) = command {
            self.adapter.apply(command)?;
        }
        Ok(())
    }
}
impl<A: PeripheralAdapter> Drop for FeedbackOutput<A> {
    fn drop(&mut self) {
        let _ = self.adapter.apply(PeripheralCommand::Stop);
    }
}

/// A project route uses channels 0..11 as (controller * 3 + motor). Original
/// keys remain independent owners; a note-off cannot quiet another held key.
pub const PROJECT_FEEDBACK_OUTPUT: &str = "windfall:project-xinput-feedback";
pub struct ProjectFeedbackOutput<A: PeripheralAdapter> {
    adapter: A,
    held: [[u16; 128]; 12],
}
impl<A: PeripheralAdapter> ProjectFeedbackOutput<A> {
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            held: [[0; 128]; 12],
        }
    }
    fn update(&mut self, controller: usize) -> Result<(), String> {
        let peak = |motor: usize| *self.held[controller * 3 + motor].iter().max().unwrap_or(&0);
        let both = peak(2);
        let low = peak(0).max(both);
        let high = peak(1).max(both);
        self.adapter.apply(PeripheralCommand::Rumble {
            controller: controller as u8,
            low,
            high,
        })
    }
}
impl<A: PeripheralAdapter> Output for ProjectFeedbackOutput<A> {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        let message = MidiMessage::parse(bytes).ok_or("invalid project feedback message")?;
        if message == MidiMessage::Stop {
            self.held.fill([0; 128]);
            return self.adapter.apply(PeripheralCommand::Stop);
        }
        let Some(channel) = message.channel().filter(|channel| *channel < 12) else {
            return Ok(());
        };
        match message {
            MidiMessage::NoteOn { key, velocity, .. } => {
                self.held[usize::from(channel)][usize::from(key)] =
                    (f32::from(velocity) / 127.0 * 65535.0).round() as u16;
            }
            MidiMessage::NoteOff { key, .. } => {
                self.held[usize::from(channel)][usize::from(key)] = 0
            }
            MidiMessage::Controller {
                number: 120 | 123, ..
            } => self.held[usize::from(channel)].fill(0),
            _ => return Ok(()),
        }
        self.update(usize::from(channel) / 3)
    }
}
impl<A: PeripheralAdapter> Drop for ProjectFeedbackOutput<A> {
    fn drop(&mut self) {
        let _ = self.adapter.apply(PeripheralCommand::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Mock {
        commands: Arc<Mutex<Vec<PeripheralCommand>>>,
        fail: bool,
    }
    impl PeripheralAdapter for Mock {
        fn apply(&mut self, command: PeripheralCommand) -> Result<(), String> {
            self.commands.lock().unwrap().push(command);
            if self.fail {
                Err("controller unplugged".into())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn worker_output_maps_notes_velocity_channels_panic_and_drop_to_mock_feedback() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let mut output = FeedbackOutput::new(Mock {
            commands: commands.clone(),
            fail: false,
        });
        output.send(&[0x90, 36, 127]).unwrap();
        output.send(&[0x90, 80, 64]).unwrap();
        output.send(&[0x80, 36, 0]).unwrap();
        output.send(&[0x91, 36, 127]).unwrap();
        output.send(&[0xb0, 123, 0]).unwrap();
        output.send(&[0xb9, 123, 0]).unwrap();
        assert!(output.send(&[0x90, 255, 127]).is_err());
        {
            let seen = commands.lock().unwrap();
            assert_eq!(
                seen[0],
                PeripheralCommand::Rumble {
                    controller: 0,
                    low: 65535,
                    high: 0
                }
            );
            assert!(
                matches!(seen[2], PeripheralCommand::Rumble { controller: 0, low: 0, high } if high > 32000)
            );
            assert_eq!(
                seen[3],
                PeripheralCommand::Rumble {
                    controller: 1,
                    low: 65535,
                    high: 0
                }
            );
            assert_eq!(
                seen[4],
                PeripheralCommand::Rumble {
                    controller: 0,
                    low: 0,
                    high: 0
                }
            );
        }
        drop(output);
        assert_eq!(
            commands.lock().unwrap().last(),
            Some(&PeripheralCommand::Stop)
        );
        let mut disconnected = FeedbackOutput::new(Mock {
            commands,
            fail: true,
        });
        assert!(disconnected.send(&[0x90, 36, 127]).is_err());
    }
    #[test]
    fn project_routes_map_low_high_and_both_without_cutting_other_held_notes() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let mut output = ProjectFeedbackOutput::new(Mock {
            commands: commands.clone(),
            fail: false,
        });
        output.send(&[0x96, 60, 127]).unwrap(); // controller 2, low
        output.send(&[0x97, 61, 64]).unwrap(); // controller 2, high
        output.send(&[0x98, 62, 32]).unwrap(); // controller 2, both
        output.send(&[0x86, 60, 0]).unwrap();
        output.send(&[0x87, 61, 0]).unwrap();
        let both = (32.0f32 / 127.0 * 65535.0).round() as u16;
        assert_eq!(
            commands.lock().unwrap()[4],
            PeripheralCommand::Rumble {
                controller: 2,
                low: both,
                high: both
            }
        );
        output.send(&[0xb8, 123, 0]).unwrap();
        assert_eq!(
            commands.lock().unwrap().last(),
            Some(&PeripheralCommand::Rumble {
                controller: 2,
                low: 0,
                high: 0
            })
        );
        assert!(output.send(&[0x90, 200, 127]).is_err());
        drop(output);
        assert_eq!(
            commands.lock().unwrap().last(),
            Some(&PeripheralCommand::Stop)
        );
        let mut failed = ProjectFeedbackOutput::new(Mock {
            commands,
            fail: true,
        });
        assert!(failed.send(&[0x90, 60, 127]).is_err());
    }
}
