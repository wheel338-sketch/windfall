//! SoundFont 2.04 synthesis units and controller algebra (spec §§8–9).
//! Prepared regions own modulation lists; the callback owns only Copy state.
use crate::{Chorus, ChorusParams, Effect, Reverb, ReverbParams};
use windfall_core::AudioBuffer;

pub(super) const GENERATORS: usize = 61;
pub(super) const INITIAL_PITCH: usize = 59;

pub(super) fn limits(op: usize) -> (f32, f32, f32) {
    match op {
        5..=7 | 10..=11 => (-12000.0, 12000.0, 0.0),
        8 => (1500.0, 13500.0, 13500.0),
        9 => (0.0, 960.0, 0.0),
        13 => (-960.0, 960.0, 0.0),
        15..=16 => (0.0, 1000.0, 0.0),
        17 => (-500.0, 500.0, 0.0),
        21 | 23 | 25 | 27 | 33 | 35 => (-12000.0, 5000.0, -12000.0),
        22 | 24 => (-16000.0, 4500.0, 0.0),
        26 | 28 | 30 | 34 | 36 | 38 => (-12000.0, 8000.0, -12000.0),
        29 => (0.0, 1000.0, 0.0),
        31..=32 | 39..=40 => (-1200.0, 1200.0, 0.0),
        37 | 48 => (0.0, 1440.0, 0.0),
        46..=47 | 58 => (-1.0, 127.0, -1.0),
        51 => (-120.0, 120.0, 0.0),
        52 => (-99.0, 99.0, 0.0),
        54 => (0.0, 3.0, 0.0),
        56 => (0.0, 1200.0, 100.0),
        57 => (0.0, 127.0, 0.0),
        _ => (-32768.0, 32767.0, 0.0),
    }
}
pub(super) fn value_generator(op: usize) -> bool {
    matches!(op, 5..=11 | 13 | 15..=17 | 21..=40 | 48 | 51..=52 | 56)
}
pub(super) fn seconds(tc: f32) -> f32 {
    if tc <= -32768.0 {
        0.0
    } else {
        (tc / 1200.0).exp2()
    }
}
pub(super) fn hertz(cents: f32) -> f32 {
    440.0 * ((cents - 6900.0) / 1200.0).exp2()
}
pub(super) fn amplitude(cb: f32) -> f32 {
    10.0_f32.powf(-cb / 200.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Modulator {
    pub source: u16,
    pub destination: u16,
    pub amount: f32,
    pub amount_source: u16,
    pub transform: u16,
    pub link: Option<(usize, usize)>,
}
impl Modulator {
    pub fn identical(self, other: Self) -> bool {
        (
            self.source,
            self.destination,
            self.amount_source,
            self.transform,
        ) == (
            other.source,
            other.destination,
            other.amount_source,
            other.transform,
        )
    }
    pub fn output(self, key: u8, velocity: u8, controls: Controls) -> f32 {
        let value = self.amount
            * source(self.source, key, velocity, controls)
            * source(self.amount_source, key, velocity, controls);
        if self.transform == 2 {
            value.abs()
        } else {
            value
        }
    }
}
pub(super) fn defaults() -> Vec<Modulator> {
    [
        (0x0502, 48, 960, 0),
        (0x0102, 8, -2400, 0),
        (0x000d, 6, 50, 0),
        (0x0081, 6, 50, 0),
        (0x0587, 48, 960, 0),
        (0x028a, 17, 1000, 0),
        (0x058b, 48, 960, 0),
        (0x00db, 16, 200, 0),
        (0x00dd, 15, 200, 0),
        (0x020e, INITIAL_PITCH as u16, 12700, 0x0010),
    ]
    .into_iter()
    .map(|(source, destination, amount, amount_source)| Modulator {
        source,
        destination,
        amount: amount as f32,
        amount_source,
        transform: 0,
        link: None,
    })
    .collect()
}
pub(super) fn valid_source(op: u16) -> bool {
    if op >> 10 > 3 {
        return false;
    }
    let index = op & 127;
    if op & 128 != 0 {
        !matches!(index, 0 | 6 | 32..=63 | 98..=101 | 120..=127)
    } else {
        matches!(index, 0 | 2 | 3 | 10 | 13 | 14 | 16 | 127)
    }
}
fn concave(x: f32) -> f32 {
    if x <= 0.0 {
        0.0
    } else {
        (1.0 + (40.0 / 96.0) * x.log10()).clamp(0.0, 1.0)
    }
}
pub(super) fn curve(op: u16, input: f32) -> f32 {
    let bipolar = op & 512 != 0;
    let reverse = op & 256 != 0;
    let kind = op >> 10;
    let shape = |x: f32| match kind {
        1 => concave(x),
        2 => 1.0 - concave(1.0 - x),
        3 => {
            if x >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        _ => x,
    };
    if bipolar {
        let x = input.clamp(-1.0, 1.0);
        let y = if kind == 3 {
            if x >= 0.0 { 1.0 } else { -1.0 }
        } else {
            x.signum() * shape(x.abs())
        };
        if reverse { -y } else { y }
    } else {
        let y = shape(input.clamp(0.0, 1.0));
        if reverse { 1.0 - y } else { y }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Controls {
    pub cc: [u8; 128],
    pub pressure: u8,
    pub poly_pressure: u8,
    pub wheel: u16,
    pub sensitivity: f32,
}
impl Default for Controls {
    fn default() -> Self {
        let mut cc = [0; 128];
        cc[7] = 127;
        cc[10] = 64;
        cc[11] = 127;
        Self {
            cc,
            pressure: 0,
            poly_pressure: 0,
            wheel: 8192,
            sensitivity: 2.0,
        }
    }
}
impl Controls {
    pub fn apply_changes(&mut self, old: Self, new: Self) {
        for (i, value) in new.cc.iter().enumerate() {
            if old.cc[i] != *value {
                self.cc[i] = *value;
            }
        }
        if old.pressure != new.pressure {
            self.pressure = new.pressure;
        }
        if old.poly_pressure != new.poly_pressure {
            self.poly_pressure = new.poly_pressure;
        }
        if old.wheel != new.wheel {
            self.wheel = new.wheel;
        }
        if old.sensitivity != new.sensitivity {
            self.sensitivity = new.sensitivity;
        }
    }
}
pub(super) fn source(op: u16, key: u8, velocity: u8, controls: Controls) -> f32 {
    if op & 255 == 0 {
        return 1.0;
    }
    let index = op & 127;
    let raw = if op & 128 != 0 {
        controls.cc[index as usize] as f32
    } else {
        match index {
            2 => velocity as f32,
            3 => key as f32,
            10 => controls.poly_pressure as f32,
            13 => controls.pressure as f32,
            14 => controls.wheel as f32 / 128.0,
            16 => controls.sensitivity,
            _ => return 0.0,
        }
    };
    // MIDI bipolar midpoint is exactly CC64 / wheel8192. The positive endpoint
    // falls just short of one, as in §9.5.3. Unipolar endpoints use 0..127.
    let input = if op & 512 != 0 {
        (raw - 64.0) / 64.0
    } else {
        raw / 127.0
    };
    curve(op, input)
}
#[derive(Debug, PartialEq)]
pub(super) struct Region {
    pub generators: [f32; GENERATORS],
    pub modulators: Box<[Modulator]>,
    pub programs: Box<[std::sync::Arc<Program>]>,
    pub root: u8,
    pub correction: i8,
    pub loop_start: u32,
    pub loop_end: u32,
    pub mode: u8,
    pub sample: usize,
    pub partner: Option<usize>,
}
impl Region {
    pub fn resolve(&self, key: u8, velocity: u8, controls: Controls) -> [f32; GENERATORS] {
        let mut values = self.generators;
        let key = if values[46] >= 0.0 {
            values[46] as u8
        } else {
            key
        };
        let velocity = if values[47] >= 0.0 {
            values[47] as u8
        } else {
            velocity
        };
        for m in &self.modulators {
            let output = if let Some((program, root)) = m.link {
                let input = self.programs[program].input(root, key, velocity, controls);
                let value = m.amount
                    * curve(m.source, input)
                    * source(m.amount_source, key, velocity, controls);
                if m.transform == 2 { value.abs() } else { value }
            } else {
                m.output(key, velocity, controls)
            };
            values[m.destination as usize] += output;
        }
        for (op, value) in values.iter_mut().enumerate() {
            if op == INITIAL_PITCH {
                continue;
            }
            // -32768 is the absolute-zero time sentinel, never clamp it to 1ms.
            if matches!(op,21|23|25..=28|30|33..=36|38) && self.generators[op] <= -32768.0 {
                *value = -32768.0;
            } else {
                let (lo, hi, _) = limits(op);
                *value = value.clamp(lo, hi);
            }
        }
        values
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct GraphNode {
    pub original: usize,
    pub modulator: Modulator,
    pub inputs: Box<[usize]>,
}
#[derive(Debug)]
pub(super) struct Program {
    pub nodes: Box<[GraphNode]>,
    pub _reservation: Option<super::BankReservation>,
}
impl PartialEq for Program {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes
    }
}
impl Program {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + std::mem::size_of_val(self.nodes.as_ref())
            + self
                .nodes
                .iter()
                .map(|n| std::mem::size_of_val(n.inputs.as_ref()))
                .sum::<usize>()
    }
    pub fn input(&self, root: usize, key: u8, velocity: u8, controls: Controls) -> f32 {
        // Hydra indices are u16. One fixed scratch array also admits arbitrarily
        // deep acyclic chains without recursion or callback heap allocation.
        let mut values = [0.0_f32; 65536];
        for node in &self.nodes {
            let m = node.modulator;
            let linked = node.inputs.iter().map(|i| values[*i]).sum::<f32>();
            if node.original == root {
                return linked;
            }
            let primary = if m.source & 255 == 127 {
                curve(m.source, linked)
            } else {
                source(m.source, key, velocity, controls)
            };
            let value = m.amount * primary * source(m.amount_source, key, velocity, controls);
            values[node.original] = if m.transform == 2 { value.abs() } else { value };
        }
        0.0
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Envelope {
    pub delay: f32,
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}
impl Envelope {
    pub fn from_values(g: &[f32; GENERATORS], volume: bool, key: f32) -> Self {
        let op = if volume { 33 } else { 25 };
        let scaled = |base: usize, scale: usize| {
            if g[base] <= -32768.0 {
                0.0
            } else {
                seconds(
                    (g[base] + g[scale] * (60.0 - key))
                        .clamp(-12000.0, if base == op + 2 { 5000.0 } else { 8000.0 }),
                )
            }
        };
        Self {
            delay: seconds(g[op]),
            attack: seconds(g[op + 1]),
            hold: scaled(op + 2, op + 6),
            decay: scaled(op + 3, op + 7),
            sustain: if volume {
                g[op + 4]
            } else {
                1.0 - g[op + 4] / 1000.0
            },
            release: seconds(g[op + 5]),
        }
    }
    pub fn held(self, t: f32, volume: bool) -> f32 {
        let t = t - self.delay;
        if t < 0.0 {
            return 0.0;
        }
        if t < self.attack {
            // Convex attack control gives an approximately linear amplitude
            // rise when interpreted by the volume amplifier's dB transfer.
            return if volume {
                t / self.attack
            } else {
                1.0 - concave(1.0 - t / self.attack)
            };
        }
        let t = t - self.attack - self.hold;
        if t < 0.0 {
            return 1.0;
        }
        let fraction = if self.decay == 0.0 {
            1.0
        } else {
            (t / self.decay).min(1.0)
        };
        if volume {
            amplitude(self.sustain * fraction)
        } else {
            1.0 + (self.sustain - 1.0) * fraction
        }
    }
}
pub(super) fn triangle(phase: f32) -> f32 {
    let p = phase.fract();
    if p < 0.25 {
        p * 4.0
    } else if p < 0.75 {
        2.0 - p * 4.0
    } else {
        p * 4.0 - 4.0
    }
}

#[derive(Clone, Copy)]
pub(super) struct State {
    pub active: bool,
    pub position: f64,
    elapsed: u64,
    releasing: bool,
    release_age: u64,
    release_values: [f32; 2],
    last_envelopes: [f32; 2],
    key: u8,
    velocity: u8,
    values: [f32; GENERATORS],
    controls: Controls,
    phase: [f32; 2],
    filter: [[f32; 2]; 2],
    quick: bool,
    pub chorus: f32,
    pub reverb: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            active: false,
            position: 0.0,
            elapsed: 0,
            releasing: false,
            release_age: 0,
            release_values: [0.0; 2],
            last_envelopes: [0.0; 2],
            key: 0,
            velocity: 0,
            values: [0.0; GENERATORS],
            controls: Controls::default(),
            phase: [0.0; 2],
            filter: [[0.0; 2]; 2],
            quick: false,
            chorus: 0.0,
            reverb: 0.0,
        }
    }
}
impl State {
    #[cfg(test)]
    pub fn values_for_test(&self) -> &[f32; GENERATORS] {
        &self.values
    }
    pub fn start(&mut self, region: &Region, key: u8, velocity: u8, controls: Controls) {
        *self = Self {
            active: true,
            key,
            velocity,
            controls,
            values: region.resolve(key, velocity, controls),
            ..Self::default()
        };
    }
    pub fn release(&mut self) {
        if !self.releasing {
            self.release_values = self.last_envelopes;
            self.release_age = 0;
            self.releasing = true;
        }
    }
    pub fn choke(&mut self) {
        self.release();
        self.quick = true;
    }
    pub fn choke_remaining(&self, rate: f32) -> usize {
        (rate * 0.001).ceil() as usize - self.release_age.min((rate * 0.001).ceil() as u64) as usize
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Inline voice rendering inputs; no callback-owned heap context"
    )]
    pub fn tick(
        &mut self,
        region: &Region,
        audio: &AudioBuffer,
        rate: f32,
        controls: Controls,
        transpose: f32,
        release_multiplier: f32,
        brightness: f32,
        note_pan: f32,
    ) -> [f32; 2] {
        if !self.active {
            return [0.0; 2];
        }
        if self.controls != controls {
            self.controls = controls;
            self.values = region.resolve(self.key, self.velocity, controls);
        }
        let g = &self.values;
        let key = if g[46] >= 0.0 { g[46] } else { self.key as f32 };
        let vol = Envelope::from_values(g, true, key);
        let env = Envelope::from_values(g, false, key);
        let t = self.elapsed as f32 / rate;
        let (volume, modulation) = if self.releasing {
            let time = self.release_age as f32 / rate;
            self.release_age += 1;
            let duration = if self.quick {
                0.001
            } else {
                vol.release * release_multiplier
            };
            let fraction = if duration == 0.0 {
                1.0
            } else {
                time / duration
            };
            if fraction >= 1.0 || self.release_values[0] == 0.0 {
                self.active = false;
                return [0.0; 2];
            }
            let volume = self.release_values[0] * amplitude(960.0 * fraction);
            let modulation =
                self.release_values[1] * (1.0 - time / env.release.max(1.0 / rate)).max(0.0);
            (volume, modulation)
        } else {
            (vol.held(t, true), env.held(t, false))
        };
        self.last_envelopes = [volume, modulation];
        let mut lfo = [0.0; 2];
        for (i, value) in lfo.iter_mut().enumerate() {
            let op = 21 + i * 2;
            if t >= seconds(g[op]) {
                *value = triangle(self.phase[i]);
                self.phase[i] = (self.phase[i] + hertz(g[op + 1]) / rate).fract();
            }
        }
        let looped = region.mode == 1 || (region.mode == 3 && !self.releasing);
        if looped && self.position >= region.loop_end as f64 {
            self.position = region.loop_start as f64
                + (self.position - region.loop_start as f64)
                    % (region.loop_end - region.loop_start) as f64;
        }
        if self.position >= audio.frames() as f64 {
            self.active = false;
            return [0.0; 2];
        }
        let frame = self.position as usize;
        let next = if looped && frame + 1 >= region.loop_end as usize {
            region.loop_start as usize
        } else {
            (frame + 1).min(audio.frames() - 1)
        };
        let (a, b) = audio.stereo_frame(frame);
        let (c, d) = audio.stereo_frame(next);
        let f = (self.position - frame as f64) as f32;
        let mut out = [a + (c - a) * f, b + (d - b) * f];
        let fc = hertz((g[8] + lfo[0] * g[10] + modulation * g[11]).clamp(1500.0, 13500.0))
            .min(40.0 + brightness * brightness * 19960.0)
            .min(rate * 0.45);
        // Stable two-pole resonant low-pass, Q in centibels, with the specified
        // half-resonance passband attenuation (§9.1.3).
        let peak = 10.0_f32.powf(g[9] / 200.0);
        let q = (0.5 * peak * peak * (1.0 + (1.0 - 1.0 / (peak * peak)).sqrt())).sqrt();
        let w = std::f32::consts::TAU * fc / rate;
        let alpha = w.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        let b0 = (1.0 - w.cos()) / (2.0 * a0);
        let b1 = 2.0 * b0;
        let a1 = -2.0 * w.cos() / a0;
        let a2 = (1.0 - alpha) / a0;
        let gain = volume * amplitude(g[48] + lfo[0] * g[13]) * amplitude(g[9] * 0.5);
        let pan = (g[17] / 500.0 + note_pan).clamp(-1.0, 1.0);
        let stereo = audio.channels() == 2;
        let pans = if stereo {
            [(1.0 - pan).min(1.0), (1.0 + pan).min(1.0)]
        } else {
            [((1.0 - pan) * 0.5).sqrt(), ((1.0 + pan) * 0.5).sqrt()]
        };
        for (i, value) in out.iter_mut().enumerate() {
            let y = b0 * *value + self.filter[i][0];
            self.filter[i][0] = b1 * *value - a1 * y + self.filter[i][1];
            self.filter[i][1] = b0 * *value - a2 * y;
            *value = y * gain * pans[i];
        }
        let cents = (key - region.root as f32) * g[56]
            + g[51] * 100.0
            + g[52]
            + region.correction as f32
            + transpose * 100.0
            + g[INITIAL_PITCH]
            + lfo[0] * g[5]
            + lfo[1] * g[6]
            + modulation * g[7];
        self.position += audio.sample_rate() as f64 / rate as f64
            * (cents.clamp(-1000000.0, 1000000.0) as f64 / 1200.0).exp2();
        self.chorus = g[15] / 1000.0;
        self.reverb = g[16] / 1000.0;
        self.elapsed += 1;
        out
    }
}

/// Defined SF2 send outputs: two post-amplifier stereo buses feed Windfall's
/// own fully-wet chorus/reverb, whose returns are summed into the rack output.
#[derive(Default)]
pub(super) struct Sends {
    chorus: Chorus,
    reverb: Reverb,
    remaining: usize,
    tail: usize,
}
impl Sends {
    pub fn prepare(&mut self, rate: f32) {
        self.chorus.set_params(&ChorusParams {
            mix: 1.0,
            ..Default::default()
        });
        self.reverb.set_params(&ReverbParams {
            mix: 1.0,
            ..Default::default()
        });
        self.chorus.prepare(rate, 1);
        self.reverb.prepare(rate, 1);
        self.tail = self.chorus.tail_samples().max(self.reverb.tail_samples());
        self.remaining = 0;
    }
    pub fn reset(&mut self) {
        self.chorus.reset();
        self.reverb.reset();
        self.remaining = 0;
    }
    pub fn tail_samples(&self) -> usize {
        self.remaining
    }
    pub fn tick(&mut self, buses: [[f32; 2]; 2]) -> [f32; 2] {
        if buses.iter().flatten().any(|v| *v != 0.0) {
            self.remaining = self.tail;
        } else {
            self.remaining = self.remaining.saturating_sub(1);
        }
        let mut cl = [buses[0][0]];
        let mut cr = [buses[0][1]];
        let mut rl = [buses[1][0]];
        let mut rr = [buses[1][1]];
        self.chorus.process(&mut cl, &mut cr);
        self.reverb.process(&mut rl, &mut rr);
        [cl[0] + rl[0], cr[0] + rr[0]]
    }
}
