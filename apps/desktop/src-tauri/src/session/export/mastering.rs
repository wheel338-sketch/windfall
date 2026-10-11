//! Mastering an export. The rendered mix is kept in a file beside the
//! export while it is measured and run through the mastering chain a few
//! times over, so a long song is never held in memory.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use windfall_dsp::mastering::{
    LoudnessMetrics, MASTERING_CEILING_RANGE, MASTERING_TARGET_RANGE, Mastering, MasteringReport,
    MasteringSettings,
};
use windfall_ipc::{LoudnessReading, MasteringOptions, MasteringReview};

/// Bytes read back from the spool at a time: 8,192 stereo frames.
const CHUNK_BYTES: usize = 8_192 * 2 * 4;

/// The rendered mix as raw 32-bit floats in a file that goes when this does.
struct Spool {
    /// `None` only while the file is being removed.
    file: Option<File>,
    path: PathBuf,
    frames: u64,
    bytes: Vec<u8>,
    samples: Vec<f32>,
}

impl Spool {
    fn create(path: PathBuf) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("the mix could not be set aside for mastering ({error})"))?;
        Ok(Self {
            file: Some(file),
            path,
            frames: 0,
            bytes: Vec::new(),
            samples: Vec::new(),
        })
    }

    fn file(&mut self) -> Result<&mut File, String> {
        self.file
            .as_mut()
            .ok_or_else(|| "the mix set aside for mastering is gone".to_owned())
    }

    fn write(&mut self, block: &[f32]) -> Result<(), String> {
        self.bytes.clear();
        for sample in block {
            self.bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.frames += (block.len() / 2) as u64;
        let Self { file, bytes, .. } = self;
        file.as_mut()
            .ok_or_else(|| "the mix set aside for mastering is gone".to_owned())?
            .write_all(bytes)
            .map_err(|error| format!("the mix could not be set aside for mastering ({error})"))
    }

    /// Hands the whole mix to `each` from the start, a chunk at a time.
    /// `each` returns false to stop early; so does this then.
    fn replay(
        &mut self,
        each: &mut dyn FnMut(&[f32]) -> Result<bool, String>,
    ) -> Result<bool, String> {
        let failed = |error: std::io::Error| {
            format!("the mix set aside for mastering could not be read ({error})")
        };
        let file = self.file()?;
        file.flush().map_err(failed)?;
        file.seek(SeekFrom::Start(0)).map_err(failed)?;
        self.bytes.resize(CHUNK_BYTES, 0);
        loop {
            let mut filled = 0;
            while filled < CHUNK_BYTES {
                let Self { file, bytes, .. } = self;
                let file = file
                    .as_mut()
                    .ok_or_else(|| "the mix set aside for mastering is gone".to_owned())?;
                let read = file.read(&mut bytes[filled..]).map_err(failed)?;
                if read == 0 {
                    break;
                }
                filled += read;
            }
            // Whole frames only: eight bytes each.
            let whole = filled - filled % 8;
            if whole == 0 {
                return Ok(true);
            }
            self.samples.clear();
            self.samples.extend(
                self.bytes[..whole]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            );
            if !each(&self.samples)? {
                return Ok(false);
            }
            if filled < CHUNK_BYTES {
                return Ok(true);
            }
        }
    }
}

impl Drop for Spool {
    fn drop(&mut self) {
        // Closed first: an open file cannot be removed everywhere.
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

fn reading(metrics: &LoudnessMetrics) -> LoudnessReading {
    LoudnessReading {
        integrated_lufs: metrics.integrated_lufs,
        loudness_range_lu: metrics.loudness_range_lu,
        momentary_max_lufs: metrics.momentary_max_lufs,
        short_term_max_lufs: metrics.short_term_max_lufs,
        true_peak_dbtp: metrics.true_peak_dbtp,
        sample_peak_db: metrics.sample_peak_db,
    }
}

fn review(report: &MasteringReport) -> MasteringReview {
    MasteringReview {
        before: reading(&report.before),
        after: reading(&report.after),
        target_lufs: report.settings.target_lufs,
        ceiling_dbtp: report.settings.ceiling_dbtp,
        gain_db: report.gain_db,
        compression_db: report.compression_db,
        limiting_db: report.limiting_db,
        passes: report.passes,
        target_reached: report.target_reached,
    }
}

/// Why an export that asks for mastering cannot start, if it cannot.
pub(super) fn check(options: &MasteringOptions, stems: bool) -> Result<(), String> {
    if stems {
        return Err(
            "Mastering works on the whole mix. Turn off stems to master the export, or turn off mastering to export stems."
                .to_owned(),
        );
    }
    let numbers = [options.target_lufs, options.ceiling_dbtp, options.dynamics];
    if numbers.iter().any(|value| !value.is_finite()) {
        return Err("The mastering target, ceiling and dynamics must be numbers.".to_owned());
    }
    if !(MASTERING_TARGET_RANGE.0..=MASTERING_TARGET_RANGE.1).contains(&options.target_lufs)
        || !(MASTERING_CEILING_RANGE.0..=MASTERING_CEILING_RANGE.1).contains(&options.ceiling_dbtp)
        || !(0.0..=1.0).contains(&options.dynamics)
    {
        return Err("Choose a loudness from -30 to -6 LUFS, a ceiling from -9 to 0 dBTP and dynamics from 0 to 1.".to_owned());
    }
    Ok(())
}

/// The mastering of one export: takes the mix as it is rendered, then
/// writes the master.
pub(super) struct MasterJob {
    spool: Spool,
    mastering: Mastering,
}

impl MasterJob {
    /// `stage` is the file the export is being written to; the mix is set
    /// aside next to it.
    pub(super) fn start(
        stage: &Path,
        sample_rate: u32,
        options: &MasteringOptions,
    ) -> Result<Self, String> {
        let settings = MasteringSettings {
            target_lufs: options.target_lufs,
            ceiling_dbtp: options.ceiling_dbtp,
            dynamics: options.dynamics,
        };
        Ok(Self {
            spool: Spool::create(stage.with_extension("master"))?,
            mastering: Mastering::new(sample_rate, settings),
        })
    }

    /// Takes the next block of the rendered mix: interleaved stereo.
    pub(super) fn take(&mut self, block: &[f32]) -> Result<(), String> {
        self.mastering.analyze(block);
        self.spool.write(block)
    }

    /// Masters what was taken and hands the master to `sink` in order.
    /// `progress` is told how far along the work is, 0 to 1, and returns
    /// false to cancel. `None` means it was cancelled.
    pub(super) fn finish(
        &mut self,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), String>,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<Option<MasteringReview>, String> {
        let Self { spool, mastering } = self;
        mastering.begin().map_err(|error| error.to_string())?;
        let total = spool.frames.max(1) as f32;
        let mut out = Vec::new();
        let mut pass = 0.0_f32;
        loop {
            let keep = mastering.is_render_pass();
            let mut done = 0.0_f32;
            let finished = spool.replay(&mut |samples| {
                mastering.process(samples, &mut out);
                if keep {
                    sink(&out)?;
                }
                done += (samples.len() / 2) as f32;
                // The number of passes is not known ahead: each is taken
                // to be the one before the last.
                Ok(progress((pass + done / total) / (pass + 2.0)))
            })?;
            if !finished {
                return Ok(None);
            }
            let report = mastering.finish_pass(&mut out);
            if keep {
                sink(&out)?;
            }
            if let Some(report) = report {
                return Ok(Some(review(&report)));
            }
            pass += 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spool_plays_back_what_it_was_given_and_removes_its_file() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("mix.master");
        let mut spool = Spool::create(path.clone()).unwrap();
        // More than one chunk, and not a whole number of them.
        let mix: Vec<f32> = (0..(8_192 * 2 * 3 + 500))
            .map(|index| (index as f32 * 0.37).sin())
            .collect();
        for block in mix.chunks(1_024) {
            spool.write(block).unwrap();
        }
        assert_eq!(spool.frames, (mix.len() / 2) as u64);
        for _ in 0..2 {
            let mut back = Vec::new();
            let finished = spool
                .replay(&mut |samples| {
                    back.extend_from_slice(samples);
                    Ok(true)
                })
                .unwrap();
            assert!(finished);
            assert_eq!(back, mix);
        }
        let mut chunks = 0;
        assert!(
            !spool
                .replay(&mut |_| {
                    chunks += 1;
                    Ok(false)
                })
                .unwrap()
        );
        assert_eq!(chunks, 1);
        assert!(path.is_file());
        drop(spool);
        assert!(!path.exists());
    }

    #[test]
    fn mastering_with_stems_or_without_numbers_is_refused() {
        let options = MasteringOptions {
            target_lufs: -14.0,
            ceiling_dbtp: -1.0,
            dynamics: 0.5,
        };
        assert_eq!(check(&options, false), Ok(()));
        assert!(check(&options, true).unwrap_err().contains("stems"));
        let broken = MasteringOptions {
            dynamics: f32::NAN,
            ..options
        };
        assert!(check(&broken, false).unwrap_err().contains("numbers"));
    }

    #[test]
    fn cancelling_during_mastering_and_encoder_failure_remove_the_spooled_mix() {
        let folder = tempfile::tempdir().unwrap();
        let options = MasteringOptions {
            target_lufs: -18.0,
            ceiling_dbtp: -1.0,
            dynamics: 0.0,
        };
        let input: Vec<f32> = (0..96_000)
            .flat_map(|n| {
                let x = (std::f32::consts::TAU * 1_000.0 * n as f32 / 48_000.0).sin() * 0.1;
                [x, x]
            })
            .collect();
        for fail_encoder in [false, true] {
            let stage = folder.path().join("export.stage");
            let mut job = MasterJob::start(&stage, 48_000, &options).unwrap();
            job.take(&input).unwrap();
            assert!(stage.with_extension("master").is_file());
            let mut written = 0;
            let mut reports = 0;
            let result = job.finish(
                &mut |block| {
                    written += block.len();
                    if fail_encoder {
                        Err("test encoder failure".to_owned())
                    } else {
                        Ok(())
                    }
                },
                &mut |_| {
                    reports += 1;
                    fail_encoder
                },
            );
            if fail_encoder {
                assert!(result.unwrap_err().contains("encoder failure"));
            } else {
                assert!(result.unwrap().is_none());
                assert_eq!(written, 0);
                assert_eq!(reports, 1);
            }
            drop(job);
            assert!(!stage.with_extension("master").exists());
        }
    }
}
