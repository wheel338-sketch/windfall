use super::{audio, output};
use crate::balance::Controls;
use crate::blocks::delay_line::DelayLine;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ts_rs::TS;

/// Admission limit, checked before PCM/FFT storage is created on the control side.
pub const MAX_IMPULSE_SAMPLES: usize = 65_536;
pub const CONVOLUTION_PARTITION: usize = 128;
const FFT_SIZE: usize = CONVOLUTION_PARTITION * 2;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ConvolverParams {
    pub mix: f32,
    pub gain: f32,
}
impl Default for ConvolverParams {
    fn default() -> Self {
        Self {
            mix: 1.0,
            gain: 1.0,
        }
    }
}
param_set!(ConvolverParams, "Convolver", {
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [gain] "gain" "Gain" { Gain, Linear, 0.0, 4.0, 1.0 }
});

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Complex {
    re: f32,
    im: f32,
}
impl Complex {
    const ZERO: Self = Self { re: 0.0, im: 0.0 };
    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
}
/// Fixed radix-2 FFT plan with no callback scratch allocation.
struct Fft {
    roots: [Complex; FFT_SIZE / 2],
}
impl Default for Fft {
    fn default() -> Self {
        Self {
            roots: std::array::from_fn(|i| {
                let a = -std::f32::consts::TAU * i as f32 / FFT_SIZE as f32;
                Complex {
                    re: a.cos(),
                    im: a.sin(),
                }
            }),
        }
    }
}
impl Fft {
    fn transform(&self, data: &mut [Complex; FFT_SIZE], inverse: bool) {
        for i in 0..FFT_SIZE {
            let j = i.reverse_bits() >> (usize::BITS - FFT_SIZE.ilog2());
            if j > i {
                data.swap(i, j);
            }
        }
        let mut width = 2;
        while width <= FFT_SIZE {
            for start in (0..FFT_SIZE).step_by(width) {
                for i in 0..width / 2 {
                    let mut root = self.roots[i * FFT_SIZE / width];
                    if inverse {
                        root.im = -root.im;
                    }
                    let a = data[start + i];
                    let b = data[start + i + width / 2].mul(root);
                    data[start + i] = Complex {
                        re: a.re + b.re,
                        im: a.im + b.im,
                    };
                    data[start + i + width / 2] = Complex {
                        re: a.re - b.re,
                        im: a.im - b.im,
                    };
                }
            }
            width *= 2;
        }
        if inverse {
            for value in data {
                value.re /= FFT_SIZE as f32;
                value.im /= FFT_SIZE as f32;
            }
        }
    }
}

/// Immutable stereo FFT partitions. Build this only on the control side.
#[derive(Debug, PartialEq)]
pub struct PreparedImpulse {
    kernels: Vec<[[Complex; FFT_SIZE]; 2]>,
    length: usize,
    group_delay: usize,
}
impl PreparedImpulse {
    /// Independent-channel stereo convolution (mono callers duplicate taps).
    /// Reject damaged data instead of silently shortening or normalizing an IR.
    pub fn prepare(frames: &[[f32; 2]]) -> Result<Self, &'static str> {
        Self::prepare_with_delay(frames, 0)
    }
    pub fn prepare_with_delay(
        frames: &[[f32; 2]],
        group_delay: usize,
    ) -> Result<Self, &'static str> {
        if group_delay >= frames.len() {
            return Err("IR group delay exceeds its support");
        }
        if frames.is_empty() || frames.len() > MAX_IMPULSE_SAMPLES {
            return Err("impulse must contain 1 to 65536 frames");
        }
        if frames
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 8.0)
        {
            return Err("impulse samples must be finite and within -8 to 8");
        }
        let fft = Fft::default();
        let mut kernels =
            vec![[[Complex::ZERO; FFT_SIZE]; 2]; frames.len().div_ceil(CONVOLUTION_PARTITION)];
        for (p, kernel) in kernels.iter_mut().enumerate() {
            for (ch, block) in kernel.iter_mut().enumerate() {
                for (i, bin) in block.iter_mut().take(CONVOLUTION_PARTITION).enumerate() {
                    bin.re = frames
                        .get(p * CONVOLUTION_PARTITION + i)
                        .map_or(0.0, |f| f[ch]);
                }
                fft.transform(block, false);
            }
        }
        Ok(Self {
            kernels,
            length: frames.len(),
            group_delay,
        })
    }
}

/// Uniform partitioned overlap-save. Assets and history are installed before
/// hosting; scalar automation never performs FFT preparation or replaces Arcs.
pub struct Convolver {
    impulse: Arc<PreparedImpulse>,
    fft: Fft,
    history: Vec<[[Complex; FFT_SIZE]; 2]>,
    previous: [[f32; CONVOLUTION_PARTITION]; 2],
    input: [[f32; CONVOLUTION_PARTITION]; 2],
    dry: [[f32; CONVOLUTION_PARTITION]; 2],
    wet: [[f32; CONVOLUTION_PARTITION]; 2],
    dry_delay: [DelayLine; 2],
    head: usize,
    cursor: usize,
    controls: Controls<2>,
}
impl Default for Convolver {
    fn default() -> Self {
        Self::with_impulse(Arc::new(
            PreparedImpulse::prepare(&[[1.0; 2]]).expect("identity IR"),
        ))
    }
}
impl Convolver {
    /// Control-side construction; replaces the whole prepared owner on IR edits.
    pub fn with_impulse(impulse: Arc<PreparedImpulse>) -> Self {
        Self {
            dry_delay: std::array::from_fn(|_| DelayLine::new(impulse.group_delay)),
            history: vec![[[Complex::ZERO; FFT_SIZE]; 2]; impulse.kernels.len()],
            impulse,
            fft: Fft::default(),
            previous: [[0.0; CONVOLUTION_PARTITION]; 2],
            input: [[0.0; CONVOLUTION_PARTITION]; 2],
            dry: [[0.0; CONVOLUTION_PARTITION]; 2],
            wet: [[0.0; CONVOLUTION_PARTITION]; 2],
            head: 0,
            cursor: 0,
            controls: Controls::new([1.0, 1.0]),
        }
    }
    fn finish_partition(&mut self) {
        let partitions = self.impulse.kernels.len();
        for ch in 0..2 {
            let block = &mut self.history[self.head][ch];
            for i in 0..CONVOLUTION_PARTITION {
                block[i] = Complex {
                    re: self.previous[ch][i],
                    im: 0.0,
                };
                block[i + CONVOLUTION_PARTITION] = Complex {
                    re: self.input[ch][i],
                    im: 0.0,
                };
            }
            self.fft.transform(block, false);
            let mut sum = [Complex::ZERO; FFT_SIZE];
            for (p, kernel) in self.impulse.kernels.iter().enumerate() {
                let past = &self.history[(self.head + partitions - p) % partitions][ch];
                for i in 0..FFT_SIZE {
                    let product = past[i].mul(kernel[ch][i]);
                    sum[i].re += product.re;
                    sum[i].im += product.im;
                }
            }
            self.fft.transform(&mut sum, true);
            for i in 0..CONVOLUTION_PARTITION {
                self.wet[ch][i] = output(sum[i + CONVOLUTION_PARTITION].re);
            }
            self.previous[ch] = self.input[ch];
            self.dry[ch] = self.input[ch];
        }
        self.head = (self.head + 1) % partitions;
    }
}
impl Effect for Convolver {
    type Params = ConvolverParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.controls.prepare(super::rate(sample_rate));
        self.reset();
    }
    fn reset(&mut self) {
        self.history.fill([[Complex::ZERO; FFT_SIZE]; 2]);
        self.previous.fill([0.0; CONVOLUTION_PARTITION]);
        self.input.fill([0.0; CONVOLUTION_PARTITION]);
        self.dry.fill([0.0; CONVOLUTION_PARTITION]);
        self.wet.fill([0.0; CONVOLUTION_PARTITION]);
        for delay in &mut self.dry_delay {
            delay.clear();
        }
        self.head = 0;
        self.cursor = 0;
        self.controls.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls.set([p.mix, p.gain]);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let [mix, gain] = self.controls.tick();
            for (ch, sample) in [l, r].into_iter().enumerate() {
                self.input[ch][self.cursor] = audio(*sample);
                let dry = if self.impulse.group_delay == 0 {
                    self.dry[ch][self.cursor]
                } else {
                    self.dry_delay[ch].tap(self.impulse.group_delay)
                };
                self.dry_delay[ch].push(self.dry[ch][self.cursor]);
                *sample = output((dry * (1.0 - mix) + self.wet[ch][self.cursor] * mix) * gain);
            }
            self.cursor += 1;
            if self.cursor == CONVOLUTION_PARTITION {
                self.finish_partition();
                self.cursor = 0;
            }
        }
    }
    fn latency_samples(&self) -> usize {
        CONVOLUTION_PARTITION + self.impulse.group_delay
    }
    fn tail_samples(&self) -> usize {
        CONVOLUTION_PARTITION + self.impulse.length - 1
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
}
