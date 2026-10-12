//! Windfall's factory sounds: a drum kit and a few bass notes, synthesized by
//! the code in this crate so nobody else can hold a claim on them. The
//! results are checked in under `content/factory` and dedicated to the
//! public domain under CC0 1.0.
//!
//! [`manifest`] lists every sound, [`DEFAULT_KIT`] names the four a new
//! project starts with, [`generate`] writes the whole folder and [`verify`]
//! checks a folder against it.
//!
//! Rendering is deterministic: the same build writes the same bytes on every
//! run, and the synthesis sticks to arithmetic that IEEE 754 defines exactly
//! so that other platforms agree too.

use std::collections::BTreeSet;
use std::path::Path;
use std::{fmt, fs, io};

use windfall_core::AudioBuffer;

use crate::dsp::Audio;
use crate::sounds::{bass, claps, cymbals, hats, kicks, percussion, snares, toms};

mod dsp;
pub mod loops;
mod math;
mod sounds;
mod wav;

#[cfg(test)]
mod analysis;
#[cfg(test)]
mod tests;

/// Sample rate of every factory sound, in hertz.
pub const SAMPLE_RATE: u32 = 48_000;

/// Bit depth of the factory WAV files.
pub const BITS_PER_SAMPLE: u16 = 24;

/// The pitch the bass sounds are recorded at, in hertz: C2, MIDI note 36,
/// with A4 at 440 Hz. A sampler whose root key is a C plays them in tune.
pub const BASS_ROOT_HZ: f64 = bass::ROOT_HZ;

/// The four sounds a new project starts with, in channel rack order: a kick,
/// a clap, a closed hat and a snare. Each entry is a display name and a path
/// relative to the factory folder, with forward slashes.
pub const DEFAULT_KIT: [(&str, &str); 4] = [
    ("Kick Punch", "Drums/Kicks/Kick Punch.wav"),
    ("Clap Wide", "Drums/Claps/Clap Wide.wav"),
    ("Hat Closed 1", "Drums/Hats/Hat Closed 1.wav"),
    ("Snare Tight", "Drums/Snares/Snare Tight.wav"),
];

/// The kind of instrument a factory sound is. Each category has its own
/// folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Kick,
    Snare,
    Clap,
    Hat,
    Cymbal,
    Tom,
    Percussion,
    Bass,
}

impl Category {
    /// Folder of this category relative to the factory folder, with forward
    /// slashes.
    pub fn folder(self) -> &'static str {
        match self {
            Category::Kick => "Drums/Kicks",
            Category::Snare => "Drums/Snares",
            Category::Clap => "Drums/Claps",
            Category::Hat => "Drums/Hats",
            Category::Cymbal => "Drums/Cymbals",
            Category::Tom => "Drums/Toms",
            Category::Percussion => "Drums/Percussion",
            Category::Bass => "Bass",
        }
    }
}

/// One factory sound.
#[derive(Debug, Clone, Copy)]
pub struct Sound {
    /// Display name. The file is named after it.
    pub name: &'static str,
    /// Path of the WAV file relative to the factory folder, with forward
    /// slashes.
    pub path: &'static str,
    /// The kind of instrument, which also decides the folder.
    pub category: Category,
    /// Level of the loudest sample in dBFS. Bright sounds sit lower so that
    /// a kit sounds balanced with every channel at the same volume.
    peak_db: f64,
    synth: fn() -> Audio,
}

impl Sound {
    /// Synthesizes the sound. This takes a few milliseconds to a few tenths
    /// of a second and always returns the same samples.
    pub fn render(&self) -> Rendered {
        let finished = dsp::finish((self.synth)(), self.peak_db);
        let channels = finished.channels();
        let mut samples = Vec::with_capacity(finished.frames() * channels.len());
        for frame in 0..finished.frames() {
            for channel in &channels {
                samples.push((channel[frame] * Rendered::FULL_SCALE).round() as i32);
            }
        }
        Rendered {
            channels: channels.len() as u16,
            samples,
        }
    }
}

/// A synthesized factory sound: signed 24-bit samples at [`SAMPLE_RATE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    channels: u16,
    samples: Vec<i32>,
}

impl Rendered {
    /// The largest value a 24-bit sample can hold.
    const FULL_SCALE: f64 = 8_388_607.0;

    /// 1 for mono, 2 for stereo.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Number of frames. One frame holds one sample per channel.
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    /// The interleaved samples, each within the signed 24-bit range.
    pub fn samples(&self) -> &[i32] {
        &self.samples
    }

    /// The sound as a complete WAV file.
    pub fn to_wav(&self) -> Vec<u8> {
        wav::encode(SAMPLE_RATE, self.channels, &self.samples)
    }

    /// The sound as decoded audio, sample for sample what a decoder reads
    /// from the WAV file.
    pub fn to_audio_buffer(&self) -> AudioBuffer {
        let data = self
            .samples
            .iter()
            .map(|&sample| sample as f32 / 8_388_608.0)
            .collect();
        AudioBuffer::from_interleaved(SAMPLE_RATE, self.channels, data)
    }
}

/// Every factory sound, in the order the browser should list them.
pub fn manifest() -> &'static [Sound] {
    &SOUNDS
}

const fn sound(
    category: Category,
    name: &'static str,
    path: &'static str,
    peak_db: f64,
    synth: fn() -> Audio,
) -> Sound {
    Sound {
        name,
        path,
        category,
        peak_db,
        synth,
    }
}

#[rustfmt::skip]
static SOUNDS: [Sound; 33] = [
    sound(Category::Kick, "Kick Punch", "Drums/Kicks/Kick Punch.wav", -1.0, kicks::punch),
    sound(Category::Kick, "Kick Deep", "Drums/Kicks/Kick Deep.wav", -1.0, kicks::deep),
    sound(Category::Kick, "Kick Tight", "Drums/Kicks/Kick Tight.wav", -1.0, kicks::tight),
    sound(Category::Kick, "Kick Hard", "Drums/Kicks/Kick Hard.wav", -1.0, kicks::hard),
    sound(Category::Kick, "Kick Soft", "Drums/Kicks/Kick Soft.wav", -2.0, kicks::soft),
    sound(Category::Snare, "Snare Tight", "Drums/Snares/Snare Tight.wav", -1.0, snares::tight),
    sound(Category::Snare, "Snare Fat", "Drums/Snares/Snare Fat.wav", -1.0, snares::fat),
    sound(Category::Snare, "Snare Noisy", "Drums/Snares/Snare Noisy.wav", -1.0, snares::noisy),
    sound(Category::Snare, "Snare Rimshot", "Drums/Snares/Snare Rimshot.wav", -1.0, snares::rimshot),
    sound(Category::Clap, "Clap Wide", "Drums/Claps/Clap Wide.wav", -1.0, claps::wide),
    sound(Category::Clap, "Clap Tight", "Drums/Claps/Clap Tight.wav", -1.0, claps::tight),
    sound(Category::Clap, "Snap", "Drums/Claps/Snap.wav", -3.0, claps::snap),
    sound(Category::Hat, "Hat Closed 1", "Drums/Hats/Hat Closed 1.wav", -6.0, hats::closed_1),
    sound(Category::Hat, "Hat Closed 2", "Drums/Hats/Hat Closed 2.wav", -6.0, hats::closed_2),
    sound(Category::Hat, "Hat Closed 3", "Drums/Hats/Hat Closed 3.wav", -6.0, hats::closed_3),
    sound(Category::Hat, "Hat Open 1", "Drums/Hats/Hat Open 1.wav", -7.0, hats::open_1),
    sound(Category::Hat, "Hat Open 2", "Drums/Hats/Hat Open 2.wav", -7.0, hats::open_2),
    sound(Category::Hat, "Hat Pedal", "Drums/Hats/Hat Pedal.wav", -8.0, hats::pedal),
    sound(Category::Cymbal, "Crash", "Drums/Cymbals/Crash.wav", -4.0, cymbals::crash),
    sound(Category::Cymbal, "Ride", "Drums/Cymbals/Ride.wav", -5.0, cymbals::ride),
    sound(Category::Tom, "Tom Low", "Drums/Toms/Tom Low.wav", -1.0, toms::low),
    sound(Category::Tom, "Tom Mid", "Drums/Toms/Tom Mid.wav", -1.0, toms::mid),
    sound(Category::Tom, "Tom High", "Drums/Toms/Tom High.wav", -1.0, toms::high),
    sound(Category::Percussion, "Rim Click", "Drums/Percussion/Rim Click.wav", -3.0, percussion::rim_click),
    sound(Category::Percussion, "Cowbell", "Drums/Percussion/Cowbell.wav", -4.0, percussion::cowbell),
    sound(Category::Percussion, "Wood Block", "Drums/Percussion/Wood Block.wav", -3.0, percussion::wood_block),
    sound(Category::Percussion, "Clave", "Drums/Percussion/Clave.wav", -4.0, percussion::clave),
    sound(Category::Percussion, "Shaker", "Drums/Percussion/Shaker.wav", -8.0, percussion::shaker),
    sound(Category::Percussion, "Conga", "Drums/Percussion/Conga.wav", -2.0, percussion::conga),
    sound(Category::Percussion, "Tambourine", "Drums/Percussion/Tambourine.wav", -6.0, percussion::tambourine),
    sound(Category::Bass, "Bass Sub", "Bass/Bass Sub.wav", -1.0, bass::sub),
    sound(Category::Bass, "Bass Drive", "Bass/Bass Drive.wav", -3.0, bass::drive),
    sound(Category::Bass, "Bass Pluck", "Bass/Bass Pluck.wav", -2.0, bass::pluck),
];

/// The two text files that travel with the sounds.
const DOCUMENTS: [(&str, &str); 2] = [
    ("LICENSE.md", include_str!("../content/LICENSE.md")),
    ("README.md", include_str!("../content/README.md")),
];

/// Every file of the factory folder with its contents, sounds first.
fn files() -> Vec<(&'static str, Vec<u8>)> {
    let sounds = SOUNDS
        .iter()
        .map(|sound| (sound.path, sound.render().to_wav()));
    let documents = DOCUMENTS
        .iter()
        .map(|&(path, text)| (path, unix_lines(text.as_bytes())));
    let loops = loops::manifest()
        .iter()
        .map(|asset| (asset.path, asset.render().to_wav()));
    sounds
        .chain(loops)
        .chain(documents)
        .chain(std::iter::once((
            "Loops/manifest.json",
            loops::metadata_json().into_bytes(),
        )))
        .collect()
}

/// Drops the carriage return of every CRLF line ending. Git may check text
/// files out with either kind of line ending, and neither is a difference
/// worth reporting.
fn unix_lines(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for (index, &byte) in text.iter().enumerate() {
        if byte != b'\r' || text.get(index + 1) != Some(&b'\n') {
            out.push(byte);
        }
    }
    out
}

/// Writes the whole factory folder into `out_dir`: every sound in
/// [`manifest`] at its path, plus `LICENSE.md` and `README.md`. Existing
/// files are overwritten. Nothing is deleted.
pub fn generate(out_dir: &Path) -> io::Result<()> {
    for (path, bytes) in files() {
        let target = out_dir.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, bytes)?;
    }
    Ok(())
}

/// One way a folder differs from what [`generate`] writes. Each variant
/// holds the path of the file relative to the folder, with forward slashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// The generator writes this file and the folder does not have it.
    Missing(String),
    /// The file exists and its contents differ.
    Different(String),
    /// The folder holds a file the generator does not write.
    Unexpected(String),
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::Missing(path) => write!(f, "missing: {path}"),
            Mismatch::Different(path) => write!(f, "different: {path}"),
            Mismatch::Unexpected(path) => write!(f, "unexpected: {path}"),
        }
    }
}

/// Checks that `dir` holds exactly what [`generate`] writes. Sounds are
/// compared byte for byte, the two text files with line endings ignored.
/// Returns every difference found; an empty list means the folder matches.
/// Fails only if the folder cannot be read.
pub fn verify(dir: &Path) -> io::Result<Vec<Mismatch>> {
    let mut found = BTreeSet::new();
    list_files(dir, "", &mut found)?;

    let mut mismatches = Vec::new();
    for (path, expected) in files() {
        if !found.remove(path) {
            mismatches.push(Mismatch::Missing(path.to_owned()));
            continue;
        }
        let mut actual = fs::read(dir.join(path))?;
        if path.ends_with(".md") {
            actual = unix_lines(&actual);
        }
        if actual != expected {
            mismatches.push(Mismatch::Different(path.to_owned()));
        }
    }
    mismatches.extend(found.into_iter().map(Mismatch::Unexpected));
    Ok(mismatches)
}

/// Collects the path of every file under `dir` into `found`, relative to the
/// folder the walk started in.
fn list_files(dir: &Path, prefix: &str, found: &mut BTreeSet<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.file_type()?.is_dir() {
            list_files(&entry.path(), &format!("{path}/"), found)?;
        } else {
            found.insert(path);
        }
    }
    Ok(())
}
