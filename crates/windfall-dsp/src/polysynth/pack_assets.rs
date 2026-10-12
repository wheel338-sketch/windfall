//! Immutable Windfall-authored content, generated on the control thread in prepare.
//! Persisted patch IDs select this versioned bank; PCM/tables never enter ParamSet.
//! All formulas and generated content in this file are dedicated to CC0-1.0.
use crate::blocks::math::key_to_hz;
use std::sync::{Arc, OnceLock};
const TABLE: usize = 512;
const MIPS: usize = 8;
const ROOTS: [u8; 6] = [36, 48, 60, 72, 84, 96];
const PCM: usize = 24000;
const SAMPLE_MIPS: usize = 4;
pub(super) struct PackAssets {
    tables: Box<[f32]>,
    samples: Box<[f32]>,
}

impl PackAssets {
    /// Called only by prepare on the control thread. Callback code already
    /// owns its strong reference and never accesses the initialization lock.
    pub fn shared() -> Arc<Self> {
        static BANK: OnceLock<Arc<PackAssets>> = OnceLock::new();
        Arc::clone(BANK.get_or_init(|| Arc::new(Self::prepare())))
    }

    pub fn prepare() -> Self {
        let mut tables = vec![0.0; 3 * 2 * MIPS * TABLE].into_boxed_slice();
        for bank in 0..3 {
            for shape in 0..2 {
                for mip in 0..MIPS {
                    for n in 0..TABLE {
                        let phase = n as f64 / TABLE as f64;
                        let mut value = 0.0;
                        for h in 1..=(128 >> mip) {
                            let base = if shape == 0 {
                                1.0 / (h * h) as f64
                            } else {
                                1.0 / h as f64
                            };
                            let weight = match bank {
                                0 => base,
                                1 => {
                                    if h % 2 == 1 {
                                        base
                                    } else {
                                        base * 0.1
                                    }
                                }
                                _ => base * ((h as f64 * 0.71).cos() * 0.4 + 0.6),
                            };
                            value += (std::f64::consts::TAU * phase * h as f64).sin() * weight;
                        }
                        tables[((bank * 2 + shape) * MIPS + mip) * TABLE + n] =
                            (value * 0.55) as f32;
                    }
                }
            }
        }
        let mut samples = vec![0.0; 3 * ROOTS.len() * SAMPLE_MIPS * PCM].into_boxed_slice();
        for bank in 0..3 {
            for (zone, &root) in ROOTS.iter().enumerate() {
                let hz = key_to_hz(root as f32) as f64;
                for mip in 0..SAMPLE_MIPS {
                    for n in 0..PCM {
                        let time = n as f64 / 48000.0;
                        let mut value = 0.0;
                        for h in 1..=(12 >> mip) {
                            if hz * h as f64 >= 22000.0 {
                                break;
                            }
                            let decay = match bank {
                                0 => 8.0,
                                1 => 12.0,
                                _ => 6.0,
                            };
                            let partial = (-(decay + 2.0 * h as f64) * time).exp() / h as f64;
                            let weight = if bank == 1 && h % 2 == 0 { 0.2 } else { 1.0 };
                            value += (std::f64::consts::TAU * hz * h as f64 * time).sin()
                                * partial
                                * weight;
                        }
                        let attack = (time / 0.003).min(1.0);
                        let end = ((PCM - n) as f64 / 240.0).min(1.0);
                        samples[((bank * ROOTS.len() + zone) * SAMPLE_MIPS + mip) * PCM + n] =
                            (value * attack * end * 0.6) as f32;
                    }
                }
            }
        }
        Self { tables, samples }
    }
    pub fn table(&self, bank: usize, phase: f32, inc: f32, scan: f32) -> f32 {
        let mut mip = 0;
        while mip + 1 < MIPS && inc * (128 >> mip) as f32 >= 0.24 {
            mip += 1;
        }
        let x = phase * TABLE as f32;
        let index = x as usize % TABLE;
        let fraction = x - index as f32;
        let read = |shape: usize| {
            let offset = ((bank * 2 + shape) * MIPS + mip) * TABLE;
            let a = self.tables[offset + index];
            let b = self.tables[offset + (index + 1) % TABLE];
            a + (b - a) * fraction
        };
        let a = read(0);
        a + (read(1) - a) * scan
    }
    pub fn sample(&self, bank: usize, key: u8, position: &mut f32, pitch: f32, rate: f32) -> f32 {
        let zone = ROOTS
            .iter()
            .enumerate()
            .min_by_key(|(_, root)| key.abs_diff(**root))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let root = key_to_hz(ROOTS[zone] as f32);
        let hz = (root * ((pitch - ROOTS[zone] as f32) / 12.0).exp2()).min(rate * 0.249);
        let step = hz / root * 48000.0 / rate;
        let mut mip = 0;
        while mip + 1 < SAMPLE_MIPS && hz * (12 >> mip) as f32 >= rate * 0.24 {
            mip += 1;
        }
        let offset = ((bank * ROOTS.len() + zone) * SAMPLE_MIPS + mip) * PCM;
        let index = *position as usize;
        let fraction = *position - index as f32;
        *position += step;
        if index >= PCM - 1 {
            return 0.0;
        }
        let a = self.samples[offset + index];
        let b = self.samples[offset + index + 1];
        a + (b - a) * fraction
    }
}

#[cfg(test)]
mod preparation_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn cold_pack_bank_preparation_is_bounded_and_complete() {
        // Call the uncached constructor directly. Another test may already
        // have initialized shared(), which would conceal the first-load cost.
        let started = Instant::now();
        let bank = PackAssets::prepare();
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(15), "cold bank: {elapsed:?}");
        assert_eq!(bank.tables.len(), 3 * 2 * MIPS * TABLE);
        assert_eq!(bank.samples.len(), 3 * ROOTS.len() * SAMPLE_MIPS * PCM);
        let bytes = (bank.tables.len() + bank.samples.len()) * std::mem::size_of::<f32>();
        assert_eq!(bytes, 7_010_304);
        assert!(bytes < 8 * 1024 * 1024);
        assert!(
            bank.tables
                .iter()
                .chain(bank.samples.iter())
                .all(|value| value.is_finite() && value.abs() < 4.0)
        );
        // Every bank, root zone and mip must contain actual content. Checking
        // the entire slab also catches an accidentally unprepared final zone.
        for table in bank.tables.chunks_exact(TABLE) {
            assert!(table.iter().any(|value| value.abs() > 0.01));
        }
        for sample in bank.samples.chunks_exact(PCM) {
            assert!(sample.iter().any(|value| value.abs() > 0.01));
        }
        eprintln!("cold own pack bank: {elapsed:?}, {bytes} bytes, all zones/mips populated");
    }
}
