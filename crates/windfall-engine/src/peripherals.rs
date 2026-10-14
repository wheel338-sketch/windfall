//! Public peripheral contract. Only the worker calls adapters; audio can
//! enqueue Copy commands through preallocated rtrb storage. No vendor SDK.
use rtrb::{Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const PERIPHERAL_CAPACITY: usize = 256;
pub const PERIPHERAL_PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeripheralCommand {
    Rumble { controller: u8, low: u16, high: u16 },
    Lighting { red: u8, green: u8, blue: u8 },
    Stop,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeripheralPacket {
    pub frame: u64,
    pub command: PeripheralCommand,
}
impl PeripheralPacket {
    /// Own little-endian test/adapter protocol. Reserved bytes must be zero.
    pub fn encode(self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0] = PERIPHERAL_PROTOCOL_VERSION;
        bytes[2..10].copy_from_slice(&self.frame.to_le_bytes());
        match self.command {
            PeripheralCommand::Rumble {
                controller,
                low,
                high,
            } => {
                bytes[1] = 1;
                bytes[10] = controller;
                bytes[11..13].copy_from_slice(&low.to_le_bytes());
                bytes[13..15].copy_from_slice(&high.to_le_bytes());
            }
            PeripheralCommand::Lighting { red, green, blue } => {
                bytes[1] = 2;
                bytes[10] = red;
                bytes[11] = green;
                bytes[12] = blue;
            }
            PeripheralCommand::Stop => bytes[1] = 3,
        }
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != 16 || bytes[0] != PERIPHERAL_PROTOCOL_VERSION || bytes[15] != 0 {
            return Err("invalid peripheral packet");
        }
        let frame = u64::from_le_bytes(
            bytes[2..10]
                .try_into()
                .map_err(|_| "truncated peripheral frame")?,
        );
        let command = match bytes[1] {
            1 if bytes[10] < 4 => PeripheralCommand::Rumble {
                controller: bytes[10],
                low: u16::from_le_bytes([bytes[11], bytes[12]]),
                high: u16::from_le_bytes([bytes[13], bytes[14]]),
            },
            2 if bytes[13..15] == [0, 0] => PeripheralCommand::Lighting {
                red: bytes[10],
                green: bytes[11],
                blue: bytes[12],
            },
            3 if bytes[10..15] == [0; 5] => PeripheralCommand::Stop,
            _ => return Err("unsupported peripheral command"),
        };
        Ok(Self { frame, command })
    }
}

/// Implementations may block/allocate. The worker is their sole caller and
/// owner, including failure, disconnect and destruction. Never call on audio.
pub trait PeripheralAdapter: Send + 'static {
    fn apply(&mut self, command: PeripheralCommand) -> Result<(), String>;
}

pub struct PeripheralIngress {
    queue: Producer<PeripheralPacket>,
    dropped: Arc<AtomicU64>,
}
impl PeripheralIngress {
    /// Single audio producer; bounded and never allocates, locks or does IO.
    pub fn push(&mut self, packet: PeripheralPacket) -> bool {
        if self.queue.push(packet).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            false
        } else {
            true
        }
    }
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

pub struct PeripheralWorker {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}
impl PeripheralWorker {
    /// `frame_now` must use the audio presentation clock, including latency.
    /// Spawn and Drop belong off audio. Hardware failure ends with Stop.
    pub fn spawn(
        mut adapter: impl PeripheralAdapter,
        mut frame_now: impl FnMut() -> u64 + Send + 'static,
    ) -> (Self, PeripheralIngress) {
        let (producer, mut consumer) = RingBuffer::new(PERIPHERAL_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let exit = stop.clone();
        let dropped = Arc::new(AtomicU64::new(0));
        let missed = dropped.clone();
        let join = std::thread::spawn(move || {
            let mut scheduler = PeripheralScheduler::default();
            let mut last = Instant::now();
            while !exit.load(Ordering::Acquire) {
                for _ in 0..PERIPHERAL_CAPACITY {
                    let Ok(packet) = consumer.pop() else { break };
                    if !scheduler.enqueue(packet) {
                        missed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if scheduler.service(frame_now(), &mut adapter).is_err() {
                    break;
                }
                if scheduler.delivered != 0 {
                    last = Instant::now();
                    scheduler.delivered = 0;
                }
                if last.elapsed() > Duration::from_secs(2) {
                    if adapter.apply(PeripheralCommand::Stop).is_err() {
                        break;
                    }
                    last = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let _ = adapter.apply(PeripheralCommand::Stop);
        });
        (
            Self {
                stop,
                join: Some(join),
            },
            PeripheralIngress {
                queue: producer,
                dropped,
            },
        )
    }
}
impl Drop for PeripheralWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub struct PeripheralScheduler {
    pending: [Option<PeripheralPacket>; PERIPHERAL_CAPACITY],
    delivered: u64,
}
impl Default for PeripheralScheduler {
    fn default() -> Self {
        Self {
            pending: [None; PERIPHERAL_CAPACITY],
            delivered: 0,
        }
    }
}
impl PeripheralScheduler {
    pub fn enqueue(&mut self, packet: PeripheralPacket) -> bool {
        if packet.command == PeripheralCommand::Stop {
            self.pending.fill(None);
            self.pending[0] = Some(packet);
            return true;
        }
        if let Some(slot) = self.pending.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(packet);
            true
        } else {
            false
        }
    }
    pub fn service(
        &mut self,
        now: u64,
        adapter: &mut impl PeripheralAdapter,
    ) -> Result<(), String> {
        // Reordered command arrival is sorted by presentation frame. Ties keep
        // queue order. Maximum quadratic work is bounded to the worker only.
        for _ in 0..PERIPHERAL_CAPACITY {
            let Some((index, packet)) = self
                .pending
                .iter()
                .enumerate()
                .filter_map(|(i, p)| p.map(|p| (i, p)))
                .filter(|(_, p)| p.frame <= now)
                .min_by_key(|(i, p)| (p.frame, *i))
            else {
                break;
            };
            self.pending[index] = None;
            if let Err(error) = adapter.apply(packet.command) {
                self.pending.fill(None);
                let _ = adapter.apply(PeripheralCommand::Stop);
                return Err(error);
            }
            self.delivered += 1;
        }
        Ok(())
    }
}

/// Fixed note ownership for a force-feedback source. Any held note contributes
/// its velocity; key 0..63 drives low motor, 64..127 drives high motor.
pub struct NoteHaptics {
    controller: u8,
    low: [u16; 64],
    high: [u16; 64],
}
impl NoteHaptics {
    pub fn new(controller: u8) -> Result<Self, &'static str> {
        if controller >= 4 {
            return Err("XInput controller must be 0..3");
        }
        Ok(Self {
            controller,
            low: [0; 64],
            high: [0; 64],
        })
    }
    pub fn note(&mut self, frame: u64, key: u8, velocity: f32) -> PeripheralPacket {
        if key < 128 {
            let value = if velocity.is_finite() {
                (velocity.clamp(0.0, 1.0) * 65535.0).round() as u16
            } else {
                0
            };
            if key < 64 {
                self.low[key as usize] = value;
            } else {
                self.high[(key - 64) as usize] = value;
            }
        }
        PeripheralPacket {
            frame,
            command: PeripheralCommand::Rumble {
                controller: self.controller,
                low: *self.low.iter().max().unwrap_or(&0),
                high: *self.high.iter().max().unwrap_or(&0),
            },
        }
    }
    pub fn stop(&mut self, frame: u64) -> PeripheralPacket {
        self.low.fill(0);
        self.high.fill(0);
        PeripheralPacket {
            frame,
            command: PeripheralCommand::Stop,
        }
    }
}

/// Converts worker-fed tempo/audio energy into RGB. This is the portable
/// lighting source; an actual vendor adapter remains a separate dependency.
pub fn lighting(frame: u64, beat_phase: f32, energy: f32) -> PeripheralPacket {
    let clean = |x: f32| {
        if x.is_finite() {
            x.clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let phase = clean(beat_phase);
    let intensity = clean(energy.sqrt());
    PeripheralPacket {
        frame,
        command: PeripheralCommand::Lighting {
            red: ((1.0 - phase) * intensity * 255.0) as u8,
            green: (phase * intensity * 255.0) as u8,
            blue: (intensity * 64.0) as u8,
        },
    }
}

#[cfg(all(target_os = "windows", feature = "xinput"))]
pub mod windows {
    use super::*;
    #[repr(C)]
    struct Vibration {
        low: u16,
        high: u16,
    }
    #[link(name = "Xinput9_1_0")]
    unsafe extern "system" {
        fn XInputSetState(index: u32, vibration: *mut Vibration) -> u32;
    }
    pub struct XInputAdapter;
    impl PeripheralAdapter for XInputAdapter {
        fn apply(&mut self, command: PeripheralCommand) -> Result<(), String> {
            let send = |controller: u8, low: u16, high: u16| {
                if controller >= 4 {
                    return Err("XInput controller must be 0..3".into());
                }
                let mut vibration = Vibration { low, high };
                // SAFETY: POD ABI matches public XINPUT_VIBRATION; pointer is
                // valid through this synchronous worker-only call.
                let code = unsafe { XInputSetState(u32::from(controller), &mut vibration) };
                if code == 0 {
                    Ok(())
                } else {
                    Err(
                        windfall_core::notice!("engine.peripherals.xinput", code = code)
                            .to_string(),
                    )
                }
            };
            match command {
                PeripheralCommand::Rumble {
                    controller,
                    low,
                    high,
                } => send(controller, low, high),
                PeripheralCommand::Stop => {
                    for controller in 0..4 {
                        let _ = send(controller, 0, 0);
                    }
                    Ok(())
                }
                PeripheralCommand::Lighting { .. } => {
                    Err("No lighting SDK is linked; supply a GPL-compatible adapter".into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Mock {
        commands: Vec<PeripheralCommand>,
        fail: bool,
    }
    impl PeripheralAdapter for Mock {
        fn apply(&mut self, command: PeripheralCommand) -> Result<(), String> {
            self.commands.push(command);
            if self.fail {
                Err("unplugged".into())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn mock_protocol_roundtrips_and_rejects_hostile_bytes() {
        for command in [
            PeripheralCommand::Rumble {
                controller: 3,
                low: 1234,
                high: 65535,
            },
            PeripheralCommand::Lighting {
                red: 1,
                green: 2,
                blue: 3,
            },
            PeripheralCommand::Stop,
        ] {
            let packet = PeripheralPacket {
                frame: u64::MAX,
                command,
            };
            assert_eq!(PeripheralPacket::decode(&packet.encode()).unwrap(), packet);
            for len in 0..16 {
                assert!(PeripheralPacket::decode(&packet.encode()[..len]).is_err());
            }
        }
        let mut bytes = PeripheralPacket {
            frame: 0,
            command: PeripheralCommand::Stop,
        }
        .encode();
        bytes[0] = 99;
        assert!(PeripheralPacket::decode(&bytes).is_err());
    }
    #[test]
    fn mock_scheduler_waits_orders_bounds_and_panic_discards_old_commands() {
        let mut scheduler = PeripheralScheduler::default();
        let mut mock = Mock::default();
        for frame in [30, 10, 20] {
            assert!(scheduler.enqueue(lighting(frame, 0.5, 1.0)));
        }
        scheduler.service(9, &mut mock).unwrap();
        assert!(mock.commands.is_empty());
        scheduler.service(20, &mut mock).unwrap();
        assert_eq!(mock.commands.len(), 2);
        scheduler.enqueue(PeripheralPacket {
            frame: 21,
            command: PeripheralCommand::Stop,
        });
        scheduler.service(100, &mut mock).unwrap();
        assert_eq!(mock.commands.len(), 3);
        assert_eq!(mock.commands[2], PeripheralCommand::Stop);
        for frame in 0..PERIPHERAL_CAPACITY {
            assert!(scheduler.enqueue(lighting(frame as u64, 0.0, 1.0)));
        }
        assert!(!scheduler.enqueue(lighting(1000, 0.0, 1.0)));
    }
    #[test]
    fn mock_disconnect_stops_and_notes_release_only_their_motor() {
        let mut notes = NoteHaptics::new(0).unwrap();
        let low = notes.note(0, 36, 1.0);
        let both = notes.note(1, 80, 0.5);
        assert_eq!(
            low.command,
            PeripheralCommand::Rumble {
                controller: 0,
                low: 65535,
                high: 0
            }
        );
        assert_eq!(
            both.command,
            PeripheralCommand::Rumble {
                controller: 0,
                low: 65535,
                high: 32768
            }
        );
        assert_eq!(
            notes.note(2, 36, 0.0).command,
            PeripheralCommand::Rumble {
                controller: 0,
                low: 0,
                high: 32768
            }
        );
        let mut scheduler = PeripheralScheduler::default();
        scheduler.enqueue(both);
        let mut mock = Mock {
            fail: true,
            ..Default::default()
        };
        assert!(scheduler.service(1, &mut mock).is_err());
        assert_eq!(mock.commands.last(), Some(&PeripheralCommand::Stop));
    }
}
