//! Opt-in acceptance checks against the audio devices of the machine they run
//! on. Nothing here runs in an ordinary test pass: every test is `#[ignore]`
//! and needs `WINDFALL_DEVICE_EVIDENCE` to name a folder for its results.
//!
//! Each test opens a real output through [`Engine`], builds a real session on
//! it, and drives the same `recording_start` / `recording_stop` path the app
//! calls. A quiet train of noise bursts on the playlist is the stimulus. The
//! kept take is decoded and every burst located by cross-correlation, which
//! gives the alignment, the drift and any discontinuity as measured numbers.
//! The numbers, what was asked of the device and what it gave, are written
//! as JSON before anything is asserted, so a failed check still leaves its
//! evidence.
//!
//! ```text
//! WINDFALL_DEVICE_EVIDENCE   folder for the JSON results (required)
//! WINDFALL_DEVICE_LABEL      name for this route in the file names
//! WINDFALL_DEVICE_HOST       audio host; default "WASAPI" on Windows
//! WINDFALL_DEVICE_OUTPUT     output device; default the system's
//! WINDFALL_DEVICE_INPUT      input device, or "Loopback: <output device>"
//! WINDFALL_DEVICE_INPUT2     a second input, for the multitrack check
//! WINDFALL_DEVICE_RATE       requested sample rate
//! WINDFALL_DEVICE_BUFFER     requested buffer frames
//! WINDFALL_DEVICE_CHANNELS   requested output channel count
//! WINDFALL_DEVICE_SECONDS    length of a take; default 12
//! WINDFALL_DEVICE_GAIN       burst amplitude, 0 to 1; default 0.05
//! WINDFALL_DEVICE_MIN_SCORE  lowest correlation that counts as a burst
//! WINDFALL_DEVICE_LOSS_SECS  how long the device-loss check waits
//! ```
//!
//! What a result means depends on the route. A loopback or a virtual cable
//! is a digital path on real driver endpoints: it measures the engine's
//! clocking, gating and take handling, not an analogue round trip. Only a
//! cable from an interface's output to its input measures converter latency.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tempfile::TempDir;
use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::{AudioBuffer, PPQ};
use windfall_engine::Engine;
use windfall_ipc::{
    AudioSettings, EngineStatus, MetronomeSettings, RecordingAlignment, RecordingLoopOptions,
    RecordingSource, RecordingState, TransportPatch,
};
use windfall_project::{
    AudioInputRoute, ClipContent, ClipId, Command, ExternalOutputRoute, MixerRecordMode,
    MixerRecording, MixerTrackPatch, Project, SamplePath, TickRange, TrackId,
};

use super::Recorder;
use crate::session::{ClipPlace, Session, SessionConfig};
use crate::settings::{SETTINGS_FILE, SettingsStore};
use crate::shell::DEV_FACTORY_DIR;

/// Seconds between the starts of two bursts.
const BURST_PERIOD: f64 = 0.5;
/// Where the first burst starts. Off the beat, so a metronome click at 120
/// BPM never lands on one.
const BURST_LEAD: f64 = 0.25;
/// Length of a burst in seconds.
const BURST_SECONDS: f64 = 0.004;
/// How far either side of where a burst belongs it is looked for.
const SEARCH_SECONDS: f64 = 0.2;

struct Options {
    evidence: PathBuf,
    label: String,
    host: Option<String>,
    output: Option<String>,
    input: Option<String>,
    input2: Option<String>,
    rate: Option<u32>,
    buffer: Option<u32>,
    channels: Option<u16>,
    seconds: f64,
    gain: f32,
    min_score: f64,
    loss_seconds: f64,
}

fn text(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn number<T: std::str::FromStr>(name: &str) -> Option<T> {
    text(name).map(|value| {
        value
            .parse()
            .unwrap_or_else(|_| panic!("{name} is not a number: {value}"))
    })
}

fn options() -> Options {
    let evidence = PathBuf::from(text("WINDFALL_DEVICE_EVIDENCE").expect(
        "set WINDFALL_DEVICE_EVIDENCE to a folder for the results; see the top of this file",
    ));
    fs::create_dir_all(&evidence).expect("the evidence folder can be made");
    Options {
        evidence,
        label: text("WINDFALL_DEVICE_LABEL").unwrap_or_else(|| "default".to_owned()),
        host: text("WINDFALL_DEVICE_HOST"),
        output: text("WINDFALL_DEVICE_OUTPUT"),
        input: text("WINDFALL_DEVICE_INPUT"),
        input2: text("WINDFALL_DEVICE_INPUT2"),
        rate: number("WINDFALL_DEVICE_RATE"),
        buffer: number("WINDFALL_DEVICE_BUFFER"),
        channels: number("WINDFALL_DEVICE_CHANNELS"),
        seconds: number("WINDFALL_DEVICE_SECONDS").unwrap_or(12.0),
        gain: number("WINDFALL_DEVICE_GAIN").unwrap_or(0.05),
        min_score: number("WINDFALL_DEVICE_MIN_SCORE").unwrap_or(0.5),
        loss_seconds: number("WINDFALL_DEVICE_LOSS_SECS").unwrap_or(40.0),
    }
}

/// A session on a real output device.
struct DeviceRig {
    session: Session,
    engine: Arc<Engine>,
    requested: AudioSettings,
    /// Holds the settings file, the stimulus and every recording.
    folder: TempDir,
}

impl DeviceRig {
    fn open(options: &Options) -> Self {
        let requested = AudioSettings {
            host: options.host.clone(),
            device: options.output.clone(),
            sample_rate: options.rate,
            buffer_frames: options.buffer,
            output_channels: options.channels,
        };
        let engine = Arc::new(Engine::start(&requested));
        let status = engine.status();
        assert!(
            status.running,
            "the output did not open: {}",
            status.error.unwrap_or_default()
        );
        let folder = tempfile::tempdir().expect("a temporary folder");
        let session = Session::new(SessionConfig {
            controller: engine.controller(),
            audio: engine.clone(),
            events: Arc::new(Recorder::default()),
            factory_dir: PathBuf::from(DEV_FACTORY_DIR),
            settings: SettingsStore::load(folder.path().join(SETTINGS_FILE)),
        });
        Self {
            session,
            engine,
            requested,
            folder,
        }
    }

    fn status(&self) -> EngineStatus {
        self.engine.status()
    }

    fn project(&self) -> Project {
        self.session.document_snapshot().project
    }

    fn host(&self) -> String {
        self.status().host
    }

    /// Writes the burst train as a WAV at the device's rate and puts it on
    /// the playlist at tick 0. Returns the clip and the mixer track it plays
    /// into.
    fn add_stimulus(&self, options: &Options, seconds: f64) -> (Stimulus, ClipId, TrackId) {
        let stimulus = Stimulus::new(self.status().sample_rate, seconds, options.gain);
        let path = self.folder.path().join("stimulus.wav");
        write_wav(
            &path,
            &AudioBuffer::from_interleaved(stimulus.rate, 2, stimulus.interleaved()),
            WavSampleFormat::Float32,
        )
        .expect("the stimulus is written");
        let before: Vec<ClipId> = self.project().playlist.clips.iter().map(|c| c.id).collect();
        self.session
            .add_audio_clip_from_file(
                &crate::paths::display(&path),
                ClipPlace {
                    track: None,
                    start: 0,
                    mixer_track: None,
                },
            )
            .expect("the stimulus clip is added");
        let project = self.project();
        let clip = project
            .playlist
            .clips
            .iter()
            .find(|clip| !before.contains(&clip.id))
            .expect("a stimulus clip");
        let ClipContent::Audio { mixer_track, .. } = &clip.content else {
            panic!("the stimulus is an audio clip");
        };
        (stimulus, clip.id, *mixer_track)
    }

    /// The input as the engine lists it, or a panic that says what it lists.
    fn input(&self, name: &str) -> windfall_ipc::RecordingInput {
        let host = self.host();
        let inputs = self.session.recording_inputs();
        inputs
            .iter()
            .find(|input| input.host == host && input.device == name)
            .cloned()
            .unwrap_or_else(|| {
                let names: Vec<_> = inputs.iter().map(|input| &input.device).collect();
                panic!("no input named {name:?} on {host}; there are {names:?}")
            })
    }

    fn source(&self, name: &str, alignment: RecordingAlignment) -> RecordingSource {
        let input = self.input(name);
        RecordingSource {
            host: input.host,
            device: input.device,
            left: 0,
            right: (input.channels > 1).then_some(1),
            alignment: Some(alignment),
            loop_recording: None,
            monitor: None,
            armed_tracks: None,
            mixer_tap: None,
        }
    }

    /// Decoded audio of the clips a project has that `before` did not.
    fn new_takes(&self, before: &Project) -> Vec<Take> {
        let project = self.project();
        project
            .playlist
            .clips
            .iter()
            .filter(|clip| !before.playlist.clips.iter().any(|old| old.id == clip.id))
            .filter_map(|clip| {
                let ClipContent::Audio {
                    sample,
                    mixer_track,
                    ..
                } = &clip.content
                else {
                    return None;
                };
                let asset = project.samples.iter().find(|asset| asset.id == *sample)?;
                let SamplePath::External(path) = &asset.path else {
                    panic!("a kept take is an external sample until the project is saved");
                };
                let audio = windfall_codec::decode_file(path).expect("the take decodes");
                Some(Take {
                    clip: clip.id,
                    start: clip.start,
                    muted: clip.muted,
                    mixer_track: *mixer_track,
                    audio,
                })
            })
            .collect()
    }
}

struct Take {
    clip: ClipId,
    start: u32,
    muted: bool,
    mixer_track: TrackId,
    audio: AudioBuffer,
}

impl Take {
    fn frames(&self) -> usize {
        self.audio.samples().len() / usize::from(self.audio.channels())
    }

    /// One channel of the take.
    fn channel(&self, index: usize) -> Vec<f32> {
        let channels = usize::from(self.audio.channels());
        self.audio
            .samples()
            .chunks_exact(channels)
            .map(|frame| frame[index.min(channels - 1)])
            .collect()
    }
}

/// A train of identical noise bursts and where each one starts.
struct Stimulus {
    rate: u32,
    gain: f32,
    frames: usize,
    burst: Vec<f32>,
    starts: Vec<usize>,
}

impl Stimulus {
    fn new(rate: u32, seconds: f64, gain: f32) -> Self {
        let length = (BURST_SECONDS * f64::from(rate)).round() as usize;
        // A fixed linear congruential sequence, so every run and every
        // machine plays the same burst.
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let burst: Vec<f32> = (0..length)
            .map(|index| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let noise = ((state >> 40) as f32 / (1_u64 << 23) as f32) - 1.0;
                let phase = std::f32::consts::TAU * index as f32 / (length - 1) as f32;
                noise * (0.5 - 0.5 * phase.cos())
            })
            .collect();
        let frames = (seconds * f64::from(rate)).round() as usize;
        let mut starts = Vec::new();
        let mut at = BURST_LEAD;
        while ((at + BURST_SECONDS) * f64::from(rate)) < frames as f64 {
            starts.push((at * f64::from(rate)).round() as usize);
            at += BURST_PERIOD;
        }
        Self {
            rate,
            gain,
            frames,
            burst,
            starts,
        }
    }

    fn interleaved(&self) -> Vec<f32> {
        let mut audio = vec![0.0; self.frames * 2];
        for start in &self.starts {
            for (index, sample) in self.burst.iter().enumerate() {
                audio[(start + index) * 2] = sample * self.gain;
                audio[(start + index) * 2 + 1] = sample * self.gain;
            }
        }
        audio
    }

    /// Where each burst is in `recorded`, a mono take that starts `origin`
    /// frames into the stimulus and repeats it every `period` frames when
    /// one is given.
    fn locate(&self, recorded: &[f32], min_score: f64) -> Measurement {
        let radius = (SEARCH_SECONDS * f64::from(self.rate)) as i64;
        let energy: f64 = self.burst.iter().map(|s| f64::from(*s).powi(2)).sum();
        let mut hits = Vec::new();
        let mut missed = 0;
        for (index, start) in self.starts.iter().enumerate() {
            let expected = *start as i64;
            if expected + self.burst.len() as i64 + radius >= recorded.len() as i64 {
                // The take ended before this burst could be heard whole.
                break;
            }
            let mut best = (0_i64, 0.0_f64);
            let mut scores = Vec::with_capacity((radius * 2 + 1) as usize);
            for lag in -radius..=radius {
                let at = expected + lag;
                let score = if at < 0 {
                    0.0
                } else {
                    let window = &recorded[at as usize..at as usize + self.burst.len()];
                    window
                        .iter()
                        .zip(&self.burst)
                        .map(|(a, b)| f64::from(*a) * f64::from(*b))
                        .sum::<f64>()
                };
                if score.abs() > best.1.abs() {
                    best = (lag, score);
                }
                scores.push(score);
            }
            let at = (expected + best.0).max(0) as usize;
            let window = &recorded[at..at + self.burst.len()];
            let heard: f64 = window.iter().map(|s| f64::from(*s).powi(2)).sum();
            let score = if heard > 0.0 {
                best.1.abs() / (energy * heard).sqrt()
            } else {
                0.0
            };
            if score < min_score {
                missed += 1;
                continue;
            }
            // A parabola through the peak and its neighbours places the
            // burst between frames.
            let peak = (best.0 + radius) as usize;
            let fraction = if peak > 0 && peak + 1 < scores.len() {
                let (a, b, c) = (
                    scores[peak - 1].abs(),
                    scores[peak].abs(),
                    scores[peak + 1].abs(),
                );
                let bend = a - 2.0 * b + c;
                if bend.abs() > f64::EPSILON {
                    (0.5 * (a - c) / bend).clamp(-0.5, 0.5)
                } else {
                    0.0
                }
            } else {
                0.0
            };
            hits.push(Hit {
                index,
                seconds: *start as f64 / f64::from(self.rate),
                offset_frames: best.0 as f64 + fraction,
                score,
                level: (heard / self.burst.len() as f64).sqrt(),
            });
        }
        Measurement::new(self.rate, hits, missed)
    }
}

struct Hit {
    index: usize,
    /// When the burst plays, in seconds from the start of the stimulus.
    seconds: f64,
    /// Frames by which the burst is late in the take. Negative is early.
    offset_frames: f64,
    /// Normalised correlation, 0 to 1.
    score: f64,
    /// RMS of the take over the burst.
    level: f64,
}

/// What the bursts of one take say.
struct Measurement {
    rate: u32,
    hits: Vec<Hit>,
    missed: usize,
    mean_frames: f64,
    min_frames: f64,
    max_frames: f64,
    /// Slope of the offsets over the take: how much the take's clock runs
    /// off the playback clock, in parts per million.
    drift_ppm: f64,
    /// Standard deviation of the offsets around that line, in frames.
    jitter_frames: f64,
    /// The largest change between two bursts in a row, in frames.
    max_step_frames: f64,
}

impl Measurement {
    fn new(rate: u32, hits: Vec<Hit>, missed: usize) -> Self {
        let count = hits.len() as f64;
        let offsets: Vec<f64> = hits.iter().map(|hit| hit.offset_frames).collect();
        let mean = offsets.iter().sum::<f64>() / count.max(1.0);
        let time_mean = hits.iter().map(|hit| hit.seconds).sum::<f64>() / count.max(1.0);
        let spread: f64 = hits.iter().map(|h| (h.seconds - time_mean).powi(2)).sum();
        let slope = if spread > 0.0 {
            hits.iter()
                .map(|h| (h.seconds - time_mean) * (h.offset_frames - mean))
                .sum::<f64>()
                / spread
        } else {
            0.0
        };
        let residual: f64 = hits
            .iter()
            .map(|h| (h.offset_frames - mean - slope * (h.seconds - time_mean)).powi(2))
            .sum();
        Self {
            rate,
            missed,
            mean_frames: mean,
            min_frames: offsets.iter().copied().fold(f64::INFINITY, f64::min),
            max_frames: offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            // Frames per second of slope, over the frames in a second.
            drift_ppm: slope / f64::from(rate) * 1e6,
            jitter_frames: (residual / count.max(1.0)).sqrt(),
            max_step_frames: offsets
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).abs())
                .fold(0.0, f64::max),
            hits,
        }
    }

    fn ms(&self, frames: f64) -> f64 {
        frames * 1000.0 / f64::from(self.rate)
    }

    fn mean_ms(&self) -> f64 {
        self.ms(self.mean_frames)
    }

    fn json(&self) -> Value {
        if self.hits.is_empty() {
            return json!({ "detected": 0, "missed": self.missed });
        }
        json!({
            "detected": self.hits.len(),
            "missed": self.missed,
            "offsetMeanFrames": round(self.mean_frames),
            "offsetMeanMs": round(self.mean_ms()),
            "offsetMinMs": round(self.ms(self.min_frames)),
            "offsetMaxMs": round(self.ms(self.max_frames)),
            "driftPpm": round(self.drift_ppm),
            "jitterMs": round(self.ms(self.jitter_frames)),
            "maxStepMs": round(self.ms(self.max_step_frames)),
            "bursts": self.hits.iter().map(|hit| json!({
                "index": hit.index,
                "atSeconds": round(hit.seconds),
                "offsetFrames": round(hit.offset_frames),
                "score": round(hit.score),
                "levelDb": round(20.0 * hit.level.max(1e-12).log10()),
            })).collect::<Vec<_>>(),
        })
    }
}

fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn alignment_json(state: &RecordingState) -> Value {
    match &state.alignment {
        Some(alignment) => json!({
            "inputSampleRate": alignment.input_sample_rate,
            "inputLatencyMs": round(alignment.input_latency_ms),
            "outputLatencyMs": round(alignment.output_latency_ms),
            "driftPpm": round(alignment.drift_ppm),
            "measuredTimestamps": alignment.measured_timestamps,
            "trimmedFrames": alignment.trimmed_frames,
        }),
        None => Value::Null,
    }
}

fn command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// What every result starts with: the build, the machine, and what was
/// asked of the device next to what it gave.
fn header(rig: &DeviceRig, options: &Options, scenario: &str) -> Value {
    let status = rig.status();
    let os_version = if cfg!(windows) {
        command_output("cmd", &["/c", "ver"])
    } else {
        command_output("uname", &["-sr"])
    };
    json!({
        "scenario": scenario,
        "label": options.label,
        "commit": command_output("git", &["rev-parse", "HEAD"]),
        "workingTreeDirty": command_output("git", &["status", "--porcelain"]).is_some(),
        "utcUnixSeconds": SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "os": std::env::consts::OS,
        "osVersion": os_version,
        "arch": std::env::consts::ARCH,
        "profile": if cfg!(debug_assertions) { "dev" } else { "release" },
        "requested": {
            "host": rig.requested.host,
            "outputDevice": rig.requested.device,
            "sampleRate": rig.requested.sample_rate,
            "bufferFrames": rig.requested.buffer_frames,
            "outputChannels": rig.requested.output_channels,
        },
        "actual": {
            "host": status.host,
            "outputDevice": status.device,
            "sampleRate": status.sample_rate,
            "bufferFrames": status.buffer_frames,
            "outputChannels": status.output_channels,
            "bufferLatencyMs": round(f64::from(status.latency_ms)),
            "processingLatencyFrames": status.latency_frames,
        },
        "inputDevice": options.input,
        "stimulus": {
            "burstSeconds": BURST_SECONDS,
            "periodSeconds": BURST_PERIOD,
            "leadSeconds": BURST_LEAD,
            "amplitude": options.gain,
            "minScore": options.min_score,
        },
    })
}

/// Writes a result and returns the checks that failed.
fn finish(options: &Options, scenario: &str, mut result: Value, checks: Vec<(String, bool)>) {
    let failed: Vec<String> = checks
        .iter()
        .filter(|(_, passed)| !passed)
        .map(|(name, _)| name.clone())
        .collect();
    result["checks"] = Value::Array(
        checks
            .iter()
            .map(|(name, passed)| json!({ "check": name, "passed": passed }))
            .collect(),
    );
    result["passed"] = Value::Bool(failed.is_empty());
    let path = options
        .evidence
        .join(format!("{scenario}-{}.json", options.label));
    fs::write(
        &path,
        serde_json::to_string_pretty(&result).expect("a result is JSON") + "\n",
    )
    .expect("the result is written");
    println!("wrote {}", path.display());
    assert!(failed.is_empty(), "{scenario} failed: {failed:?}");
}

fn check(checks: &mut Vec<(String, bool)>, name: impl Into<String>, passed: bool) {
    checks.push((name.into(), passed));
}

/// Polls a running take until `seconds` of it are written, and returns the
/// last state and the worst the stream reported on the way.
fn record_for(rig: &DeviceRig, seconds: f64, patience: Duration) -> (RecordingState, Vec<Value>) {
    let rate = f64::from(rig.status().sample_rate);
    let deadline = Instant::now() + patience;
    let started = Instant::now();
    let mut samples = Vec::new();
    loop {
        let state = rig.session.recording_state();
        let stats = rig.engine.controller().stream_stats();
        if samples.len() < 400 {
            samples.push(json!({
                "atMs": started.elapsed().as_millis() as u64,
                "frames": state.frames,
                "xruns": stats.xruns,
                "error": state.error,
                "countInRemaining": rig.session.transport_state().count_in_remaining,
                "alignment": alignment_json(&state),
            }));
        }
        if state.error.is_some() || state.frames as f64 >= seconds * rate {
            return (state, samples);
        }
        assert!(
            Instant::now() < deadline,
            "the take wrote {} frames in {patience:?}",
            state.frames
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// One synchronized take of the stimulus through `input`.
fn synchronized_take(
    rig: &DeviceRig,
    options: &Options,
    stimulus: &Stimulus,
    input: &str,
    offset_ms: f64,
    count_in_bars: u8,
) -> (Value, Option<Measurement>, Option<Take>) {
    rig.session
        .transport_set(TransportPatch {
            count_in_bars: Some(count_in_bars),
            metronome: Some(MetronomeSettings {
                enabled: false,
                gain: options.gain.min(0.25),
                accent: true,
            }),
            ..TransportPatch::default()
        })
        .expect("the count-in is set");
    let before = rig.project();
    let transport_before = rig.session.transport_state();
    let region_before = rig.session.controller().timeline_region();
    let xruns_before = rig.engine.controller().stream_stats().xruns;
    let source = rig.source(
        input,
        RecordingAlignment {
            synchronize: true,
            drift_correction: true,
            offset_ms,
            input_sample_rate: None,
        },
    );
    let called = Instant::now();
    let started = rig.session.recording_start(source, 0, None);
    let open_ms = called.elapsed().as_millis() as u64;
    let mut result = json!({
        "offsetMs": offset_ms,
        "countInBars": count_in_bars,
        "openMs": open_ms,
    });
    if let Err(error) = started {
        result["startError"] = Value::String(error);
        return (result, None, None);
    }
    let seconds = stimulus.frames as f64 / f64::from(stimulus.rate);
    let (state, samples) = record_for(rig, seconds, Duration::from_secs_f64(seconds + 40.0));
    let first_audio_ms = samples
        .iter()
        .find(|sample| sample["frames"].as_u64().unwrap_or(0) > 0)
        .and_then(|sample| sample["atMs"].as_u64());
    result["firstWrittenAtMs"] = json!(first_audio_ms);
    result["alignment"] = alignment_json(&state);
    result["progress"] = Value::Array(samples);
    let stopped = Instant::now();
    let kept = rig.session.recording_stop();
    result["stopMs"] = json!(stopped.elapsed().as_millis() as u64);
    result["xrunsDuringTake"] = json!(rig.engine.controller().stream_stats().xruns - xruns_before);
    let transport_after = rig.session.transport_state();
    result["transportRestored"] = json!(
        transport_after.mode == transport_before.mode
            && transport_after.loop_song == transport_before.loop_song
            && rig.session.controller().timeline_region() == region_before
            && !transport_after.playing
    );
    if let Err(error) = kept {
        result["stopError"] = Value::String(error);
        return (result, None, None);
    }
    let mut takes = rig.new_takes(&before);
    assert_eq!(takes.len(), 1, "one take becomes one clip");
    let take = takes.remove(0);
    let measurement = stimulus.locate(&take.channel(0), options.min_score);
    result["takeFrames"] = json!(take.frames());
    result["takeSampleRate"] = json!(take.audio.sample_rate());
    result["takeChannels"] = json!(take.audio.channels());
    result["takeStartTick"] = json!(take.start);
    result["measurement"] = measurement.json();
    (result, Some(measurement), Some(take))
}

fn required_input(options: &Options) -> String {
    options
        .input
        .clone()
        .expect("set WINDFALL_DEVICE_INPUT to the input to record")
}

#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_inventory() {
    let options = options();
    let rig = DeviceRig::open(&options);
    let mut result = header(&rig, &options, "inventory");
    let hosts = rig.session.engine_devices();
    let inputs = rig.session.recording_inputs();
    result["hosts"] = serde_json::to_value(&hosts).expect("hosts are JSON");
    result["inputs"] = serde_json::to_value(&inputs).expect("inputs are JSON");
    let mut checks = Vec::new();
    check(&mut checks, "an output host is listed", !hosts.is_empty());
    check(
        &mut checks,
        "the open device is one of the listed devices",
        hosts.iter().any(|host| {
            host.name == rig.host()
                && host
                    .devices
                    .iter()
                    .any(|device| Some(&device.name) == rig.status().device.as_ref())
        }),
    );
    check(
        &mut checks,
        "every listed input has channels and a rate",
        inputs
            .iter()
            .all(|input| input.channels > 0 && !input.sample_rates.is_empty()),
    );
    if cfg!(windows) {
        check(
            &mut checks,
            "each WASAPI output is also listed as a loopback input",
            hosts
                .iter()
                .filter(|host| host.name == "WASAPI")
                .all(|host| {
                    host.devices.iter().all(|device| {
                        let name = windfall_engine::recording_endpoint::loopback_name(&device.name);
                        inputs
                            .iter()
                            .any(|input| input.host == host.name && input.device == name)
                    })
                }),
        );
    }
    finish(&options, "inventory", result, checks);
}

/// Plays the bursts, records them back synchronized, and measures where they
/// land. A second take applies the measured offset as the manual recording
/// offset and must land on the grid.
#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_round_trip_alignment() {
    let options = options();
    let input = required_input(&options);
    let rig = DeviceRig::open(&options);
    let (stimulus, _, _) = rig.add_stimulus(&options, options.seconds);
    let mut result = header(&rig, &options, "round-trip");
    let mut checks = Vec::new();

    let before = rig.project();
    let (raw, measured, _) = synchronized_take(&rig, &options, &stimulus, &input, 0.0, 0);
    result["raw"] = raw;
    let Some(measured) = measured else {
        check(&mut checks, "the uncalibrated take is kept", false);
        return finish(&options, "round-trip", result, checks);
    };
    let expected = stimulus
        .starts
        .iter()
        .filter(|start| {
            (**start + stimulus.burst.len()) as f64 + SEARCH_SECONDS * f64::from(stimulus.rate)
                < result["raw"]["takeFrames"].as_u64().unwrap_or(0) as f64
        })
        .count();
    check(
        &mut checks,
        "every burst is found in the take",
        measured.missed == 0 && measured.hits.len() == expected && expected > 2,
    );
    check(
        &mut checks,
        "the bursts keep their spacing: jitter at most 1 ms",
        measured.ms(measured.jitter_frames) <= 1.0,
    );
    check(
        &mut checks,
        "no dropped or repeated block: no step over 1 ms between bursts",
        measured.ms(measured.max_step_frames) <= 1.0,
    );
    check(
        &mut checks,
        "residual drift within 50 ppm",
        measured.drift_ppm.abs() <= 50.0,
    );
    check(
        &mut checks,
        "the systematic offset is within the manual offset's range",
        measured.mean_ms().abs() < 1000.0,
    );
    check(
        &mut checks,
        "stopping restores the transport and the region",
        result["raw"]["transportRestored"] == json!(true),
    );

    // One undo removes the take; redo brings it back.
    let with_take = rig.project();
    rig.session.undo().expect("the take can be undone");
    check(
        &mut checks,
        "undo removes the kept take's clip",
        rig.project().playlist.clips.len() == before.playlist.clips.len(),
    );
    rig.session.redo().expect("the take can be redone");
    check(
        &mut checks,
        "redo restores it",
        rig.project().playlist.clips == with_take.playlist.clips,
    );
    rig.session.undo().expect("the take is undone again");

    // The measured offset, applied as the manual recording offset. A
    // positive offset advances the take, so audio that arrived late moves
    // back onto the grid.
    let calibration = measured.mean_ms();
    let (calibrated, remeasured, _) =
        synchronized_take(&rig, &options, &stimulus, &input, calibration, 0);
    result["calibrated"] = calibrated;
    match remeasured {
        Some(remeasured) => {
            check(
                &mut checks,
                "with the measured offset applied the bursts land within 1 ms",
                remeasured.missed == 0 && remeasured.mean_ms().abs() <= 1.0,
            );
        }
        None => check(&mut checks, "the calibrated take is kept", false),
    }
    result["xrunsTotal"] = json!(rig.engine.controller().stream_stats().xruns);
    finish(&options, "round-trip", result, checks);
}

/// A bar of count-in comes first. The take must start at the song's first
/// tick all the same: the bursts land where they do with no count-in.
#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_count_in_gate() {
    let options = options();
    let input = required_input(&options);
    let rig = DeviceRig::open(&options);
    let seconds = options.seconds.min(6.0);
    let (stimulus, _, _) = rig.add_stimulus(&options, seconds);
    let mut result = header(&rig, &options, "count-in");
    let mut checks = Vec::new();
    let tempo = rig.project().settings.tempo_bpm;
    result["tempoBpm"] = json!(tempo);

    let (plain, reference, _) = synchronized_take(&rig, &options, &stimulus, &input, 0.0, 0);
    result["withoutCountIn"] = plain;
    rig.session.undo();
    let (counted, measured, take) = synchronized_take(&rig, &options, &stimulus, &input, 0.0, 1);
    result["withCountIn"] = counted;
    let (Some(reference), Some(measured), Some(take)) = (reference, measured, take) else {
        check(&mut checks, "both takes are kept", false);
        return finish(&options, "count-in", result, checks);
    };
    // Four beats at the project's tempo, the default signature being 4/4.
    let bar_ms = 4.0 * 60_000.0 / tempo;
    let delay = result["withCountIn"]["firstWrittenAtMs"]
        .as_u64()
        .unwrap_or(0) as f64
        - result["withoutCountIn"]["firstWrittenAtMs"]
            .as_u64()
            .unwrap_or(0) as f64;
    result["countInDelayMs"] = json!(round(delay));
    result["expectedCountInMs"] = json!(round(bar_ms));
    check(
        &mut checks,
        "audio is first written about one bar later than without a count-in",
        (delay - bar_ms).abs() <= 400.0,
    );
    check(
        &mut checks,
        "every burst is found after the count-in",
        measured.missed == 0 && measured.hits.len() == reference.hits.len(),
    );
    check(
        &mut checks,
        "the count-in does not move the take: within 1 ms of the take without one",
        (measured.mean_ms() - reference.mean_ms()).abs() <= 1.0,
    );
    // The last count-in click sounds one beat before the gate opens. None
    // of the pre-roll may be in the take: up to the first burst it holds
    // nothing louder than a tenth of a burst.
    let lead = (BURST_LEAD * f64::from(stimulus.rate) * 0.8) as usize;
    let audio = take.channel(0);
    let quiet = audio[..lead.min(audio.len())]
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    let burst_peak = audio
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    result["leadPeak"] = json!(quiet);
    result["burstPeak"] = json!(burst_peak);
    check(
        &mut checks,
        "nothing of the count-in is in the take",
        burst_peak > 0.0 && quiet < burst_peak * 0.1,
    );
    check(
        &mut checks,
        "the pre-roll was trimmed from the capture",
        result["withCountIn"]["alignment"]["trimmedFrames"]
            .as_u64()
            .unwrap_or(0)
            > result["withoutCountIn"]["alignment"]["trimmedFrames"]
                .as_u64()
                .unwrap_or(0),
    );
    check(
        &mut checks,
        "stopping restores the transport and the region",
        result["withCountIn"]["transportRestored"] == json!(true),
    );
    finish(&options, "count-in", result, checks);
}

/// Loops one bar and keeps every pass. Each complete pass must be exactly
/// one region long and hold the bursts at the same places.
#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_loop_takes() {
    let options = options();
    let input = required_input(&options);
    let rig = DeviceRig::open(&options);
    let tempo = rig.project().settings.tempo_bpm;
    let bar_seconds = 4.0 * 60.0 / tempo;
    let (stimulus, _, _) = rig.add_stimulus(&options, bar_seconds);
    let rate = f64::from(stimulus.rate);
    let mut result = header(&rig, &options, "loop-takes");
    let mut checks = Vec::new();
    let region = TickRange {
        start: 0,
        end: PPQ * 4,
    };
    result["tempoBpm"] = json!(tempo);
    result["regionTicks"] = json!([region.start, region.end]);
    result["regionFrames"] = json!(bar_seconds * rate);

    rig.session
        .transport_set(TransportPatch {
            count_in_bars: Some(0),
            ..TransportPatch::default()
        })
        .expect("no count-in");
    let before = rig.project();
    let transport_before = rig.session.transport_state();
    let mut source = rig.source(
        &input,
        RecordingAlignment {
            synchronize: true,
            drift_correction: true,
            offset_ms: 0.0,
            input_sample_rate: None,
        },
    );
    source.loop_recording = Some(RecordingLoopOptions { region });
    rig.session
        .recording_start(source, region.start, None)
        .expect("loop recording starts");
    // Three passes and most of a fourth.
    let (state, _) = record_for(&rig, bar_seconds * 3.6, Duration::from_secs(60));
    result["alignment"] = alignment_json(&state);
    result["takesWhileRecording"] = serde_json::to_value(&state.takes).expect("takes are JSON");
    let kept = rig.session.recording_stop();
    if let Err(error) = &kept {
        result["stopError"] = json!(error);
        check(&mut checks, "the loop passes are kept", false);
        return finish(&options, "loop-takes", result, checks);
    }
    let after = rig.project();
    let mut takes = rig.new_takes(&before);
    takes.sort_by_key(|take| take.clip);
    // A region that is not a whole number of frames is cut at rounded
    // running totals, so a pass may be a frame longer or shorter than the
    // region and the passes still add up exactly.
    let region_frames = (bar_seconds * rate).round() as usize;
    let whole = |take: &Take| take.frames().abs_diff(region_frames) <= 1;
    let mut means = Vec::new();
    let mut passes = Vec::new();
    for take in &takes {
        let measurement = stimulus.locate(&take.channel(0), options.min_score);
        passes.push(json!({
            "frames": take.frames(),
            "startTick": take.start,
            "muted": take.muted,
            "measurement": measurement.json(),
        }));
        if whole(take) && !measurement.hits.is_empty() {
            means.push(measurement.mean_ms());
        }
    }
    result["passes"] = Value::Array(passes);
    let complete = takes.iter().filter(|take| whole(take)).count();
    let total: usize = takes.iter().take(3).map(Take::frames).sum();
    result["threePassFrames"] = json!(total);
    check(
        &mut checks,
        "three complete passes and a final partial one are kept",
        takes.len() == 4 && complete == 3 && takes[3].frames() + 1 < region_frames,
    );
    check(
        &mut checks,
        "three passes together are three regions long, to the frame",
        total == (bar_seconds * rate * 3.0).round() as usize,
    );
    check(
        &mut checks,
        "every pass starts on the region's first tick",
        takes.iter().all(|take| take.start == region.start),
    );
    check(
        &mut checks,
        "earlier passes are muted and the latest is audible",
        takes.len() == 4 && takes[..3].iter().all(|take| take.muted) && !takes[3].muted,
    );
    let spread = means.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - means.iter().copied().fold(f64::INFINITY, f64::min);
    result["passMeanSpreadMs"] = json!(round(spread));
    check(
        &mut checks,
        "the bursts sit at the same place in every complete pass, within 1 ms",
        means.len() == 3 && spread <= 1.0,
    );
    let group = after.playlist.take_groups.last();
    result["takeGroup"] = serde_json::to_value(group).expect("a take group is JSON");
    check(
        &mut checks,
        "the passes are saved as one take group",
        after.playlist.take_groups.len() == before.playlist.take_groups.len() + 1
            && group.is_some_and(|group| group.lanes.len() == 1 && group.lanes[0].takes.len() == 4),
    );
    rig.session.undo().expect("the passes can be undone");
    let undone = rig.project();
    check(
        &mut checks,
        "one undo removes every pass and the group",
        undone.playlist.clips.len() == before.playlist.clips.len()
            && undone.playlist.take_groups.len() == before.playlist.take_groups.len(),
    );
    let transport_after = rig.session.transport_state();
    check(
        &mut checks,
        "stopping restores the loop setting and the mode",
        transport_after.loop_song == transport_before.loop_song
            && transport_after.mode == transport_before.mode,
    );
    finish(&options, "loop-takes", result, checks);
}

/// Arms a track on the hardware input and the stimulus's own track after
/// its fader, records both at once, and compares them.
#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_multitrack_arms() {
    let options = options();
    let input = required_input(&options);
    let rig = DeviceRig::open(&options);
    let seconds = options.seconds.min(8.0);
    let (stimulus, _, stimulus_track) = rig.add_stimulus(&options, seconds);
    let mut result = header(&rig, &options, "multitrack");
    let mut checks = Vec::new();

    let arm_input = |name: &str, device: &str| -> TrackId {
        let listed = rig.input(device);
        let created = rig
            .session
            .dispatch(
                Command::AddMixerTrack {
                    name: Some(name.to_owned()),
                },
                None,
            )
            .expect("a mixer track is added");
        let track = TrackId(created.created[0]);
        rig.session
            .dispatch(
                Command::UpdateMixerTrack {
                    id: track,
                    patch: MixerTrackPatch {
                        recording: Some(MixerRecording {
                            input: Some(AudioInputRoute {
                                host: listed.host,
                                device: listed.device,
                                left: 0,
                                right: (listed.channels > 1).then_some(1),
                            }),
                            armed: true,
                            ..MixerRecording::default()
                        }),
                        ..MixerTrackPatch::default()
                    },
                },
                None,
            )
            .expect("the input is armed");
        track
    };
    let mut hardware = vec![(arm_input("Device input", &input), input.clone())];
    if let Some(second) = &options.input2 {
        hardware.push((arm_input("Second device input", second), second.clone()));
    }
    rig.session
        .dispatch(
            Command::UpdateMixerTrack {
                id: stimulus_track,
                patch: MixerTrackPatch {
                    recording: Some(MixerRecording {
                        armed: true,
                        mode: MixerRecordMode::PostFader,
                        ..MixerRecording::default()
                    }),
                    ..MixerTrackPatch::default()
                },
            },
            None,
        )
        .expect("the stimulus track is armed after its fader");
    rig.session
        .transport_set(TransportPatch {
            count_in_bars: Some(0),
            ..TransportPatch::default()
        })
        .expect("no count-in");

    let before = rig.project();
    let started = rig.session.recording_start(
        RecordingSource {
            host: String::new(),
            device: String::new(),
            left: 0,
            right: None,
            alignment: Some(RecordingAlignment {
                synchronize: true,
                drift_correction: true,
                offset_ms: 0.0,
                input_sample_rate: None,
            }),
            loop_recording: None,
            monitor: None,
            armed_tracks: Some(true),
            mixer_tap: None,
        },
        0,
        None,
    );
    if let Err(error) = &started {
        result["startError"] = json!(error);
        check(&mut checks, "the armed group starts", false);
        return finish(&options, "multitrack", result, checks);
    }
    let (state, _) = record_for(&rig, seconds, Duration::from_secs_f64(seconds + 40.0));
    result["tracksWhileRecording"] = Value::Array(
        state
            .tracks
            .iter()
            .flatten()
            .map(|track| {
                json!({
                    "name": track.name,
                    "frames": track.frames,
                    "inputSampleRate": track.alignment.input_sample_rate,
                    "inputLatencyMs": round(track.alignment.input_latency_ms),
                    "outputLatencyMs": round(track.alignment.output_latency_ms),
                    "driftPpm": round(track.alignment.drift_ppm),
                    "measuredTimestamps": track.alignment.measured_timestamps,
                })
            })
            .collect(),
    );
    let kept = rig.session.recording_stop();
    if let Err(error) = &kept {
        result["stopError"] = json!(error);
        check(&mut checks, "the armed group is kept", false);
        return finish(&options, "multitrack", result, checks);
    }
    let after = rig.project();
    let takes = rig.new_takes(&before);
    let mut tracks = Vec::new();
    let mut tap_mean = None;
    let mut device_means = Vec::new();
    for take in &takes {
        let measurement = stimulus.locate(&take.channel(0), options.min_score);
        let name = after
            .mixer
            .track(take.mixer_track)
            .map(|track| track.name.clone());
        let is_tap = !hardware.iter().any(|(track, _)| *track == take.mixer_track);
        tracks.push(json!({
            "mixerTrack": name,
            "source": if is_tap { "post-fader mixer tap" } else { "hardware input" },
            "frames": take.frames(),
            "measurement": measurement.json(),
        }));
        if !measurement.hits.is_empty() {
            if is_tap {
                tap_mean = Some((measurement.mean_frames, measurement.missed));
            } else {
                device_means.push(measurement.mean_ms());
            }
        }
    }
    result["tracks"] = Value::Array(tracks);
    check(
        &mut checks,
        "every armed track is kept as its own clip",
        takes.len() == hardware.len() + 1,
    );
    check(
        &mut checks,
        "all tracks of the group have the same length",
        takes
            .windows(2)
            .all(|pair| pair[0].frames() == pair[1].frames()),
    );
    check(
        &mut checks,
        "the mixer tap holds the stimulus within one frame of where it plays",
        tap_mean.is_some_and(|(mean, missed)| missed == 0 && mean.abs() <= 1.0),
    );
    check(
        &mut checks,
        "each hardware input holds every burst",
        device_means.len() == hardware.len(),
    );
    check(
        &mut checks,
        "the group is saved as one take group with a lane for each track",
        after.playlist.take_groups.len() == before.playlist.take_groups.len() + 1
            && after
                .playlist
                .take_groups
                .last()
                .is_some_and(|group| group.lanes.len() == takes.len()),
    );
    rig.session.undo().expect("the group can be undone");
    check(
        &mut checks,
        "one undo removes every track of the group",
        rig.project().playlist.clips.len() == before.playlist.clips.len(),
    );
    finish(&options, "multitrack", result, checks);
}

/// Listens to the input through a mixer track with nothing recording, and
/// reports what the monitor queue does. Choose a route that cannot feed
/// back: the input must not hear the output.
#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_input_monitoring() {
    let options = options();
    let input = required_input(&options);
    let rig = DeviceRig::open(&options);
    let listed = rig.input(&input);
    let mut result = header(&rig, &options, "monitoring");
    let mut checks = Vec::new();
    let created = rig
        .session
        .dispatch(
            Command::AddMixerTrack {
                name: Some("Monitor".to_owned()),
            },
            None,
        )
        .expect("a mixer track is added");
    rig.session
        .dispatch(
            Command::UpdateMixerTrack {
                id: TrackId(created.created[0]),
                patch: MixerTrackPatch {
                    recording: Some(MixerRecording {
                        input: Some(AudioInputRoute {
                            host: listed.host,
                            device: listed.device,
                            left: 0,
                            right: (listed.channels > 1).then_some(1),
                        }),
                        monitor: true,
                        monitor_gain: 0.1,
                        ..MixerRecording::default()
                    }),
                    ..MixerTrackPatch::default()
                },
            },
            None,
        )
        .expect("monitoring is enabled on the track");
    let started = rig.session.input_monitor_start();
    if let Err(error) = &started {
        result["startError"] = json!(error);
        check(&mut checks, "monitoring starts", false);
        return finish(&options, "monitoring", result, checks);
    }
    let mut samples = Vec::new();
    let began = Instant::now();
    let mut last = None;
    while began.elapsed() < Duration::from_secs_f64(options.seconds.min(8.0)) {
        std::thread::sleep(Duration::from_millis(250));
        let state = rig.session.input_monitor_state();
        samples.push(json!({
            "atMs": began.elapsed().as_millis() as u64,
            "active": state.active,
            "error": state.error,
            "tracks": state.tracks.iter().map(|track| json!({
                "inputSampleRate": track.alignment.input_sample_rate,
                "driftPpm": round(track.alignment.drift_ppm),
                "bufferedMs": round(track.monitor.buffered_ms),
                "droppedFrames": track.monitor.dropped_frames,
                "starvedFrames": track.monitor.starved_frames,
            })).collect::<Vec<_>>(),
        }));
        last = Some(state);
    }
    result["samples"] = Value::Array(samples.clone());
    let last = last.expect("monitoring was polled");
    check(
        &mut checks,
        "monitoring stays active with no error",
        last.active && last.error.is_none(),
    );
    check(
        &mut checks,
        "the input reports its rate",
        last.tracks
            .iter()
            .all(|track| track.alignment.input_sample_rate > 0),
    );
    // Starvation while the queue primes is expected. Once it has run for
    // two seconds the count must stand still.
    let starved = |sample: &Value| {
        sample["tracks"][0]["starvedFrames"]
            .as_u64()
            .unwrap_or(u64::MAX)
    };
    let settled: Vec<u64> = samples
        .iter()
        .filter(|sample| sample["atMs"].as_u64().unwrap_or(0) >= 2_000)
        .map(starved)
        .collect();
    check(
        &mut checks,
        "no underrun once the monitor queue has settled",
        settled.len() > 2 && settled.first() == settled.last(),
    );
    let dropped = last
        .tracks
        .iter()
        .map(|track| track.monitor.dropped_frames)
        .sum::<u64>();
    check(&mut checks, "no monitor frames are dropped", dropped == 0);
    let stopped = rig.session.input_monitor_stop();
    check(&mut checks, "stopping ends the group", !stopped.active);
    result["xrunsTotal"] = json!(rig.engine.controller().stream_stats().xruns);
    finish(&options, "monitoring", result, checks);
}

/// Sends the stimulus's track to a hardware port pair of its own and records
/// the device's loopback of that pair and of the first pair.
#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_external_output_ports() {
    let options = options();
    let input = required_input(&options);
    let rig = DeviceRig::open(&options);
    let seconds = options.seconds.min(5.0);
    let (stimulus, _, track) = rig.add_stimulus(&options, seconds);
    let mut result = header(&rig, &options, "external-ports");
    let mut checks = Vec::new();
    let channels = rig.status().output_channels;
    let listed = rig.input(&input);
    result["inputChannels"] = json!(listed.channels);
    if channels < 2 || listed.channels < 2 {
        result["skipped"] = json!(format!(
            "needs an output and a loopback of at least two channels; the device opened with {channels} and the input has {}",
            listed.channels
        ));
        check(&mut checks, "a route with two ports is available", false);
        return finish(&options, "external-ports", result, checks);
    }
    // With four ports the track gets the second pair to itself. A stereo
    // device can still show that an assignment reaches the port it names
    // and no other: the track is folded to mono on port 2 alone.
    let wide = channels >= 4 && listed.channels >= 4;
    result["mode"] = json!(if wide {
        "stereo pair on ports 3 and 4"
    } else {
        "mono fold on port 2 of a two-port device"
    });
    let (assigned, other) = if wide {
        ((2_u16, Some(3_u16)), (0_u16, Some(1_u16)))
    } else {
        ((1, None), (0, None))
    };
    rig.session
        .dispatch(
            Command::SetTrackExternalOutput {
                id: track,
                route: Some(ExternalOutputRoute {
                    left: assigned.0,
                    right: assigned.1,
                    exclusive: true,
                }),
            },
            None,
        )
        .expect("the track is sent to its own ports");
    rig.session
        .transport_set(TransportPatch {
            count_in_bars: Some(0),
            ..TransportPatch::default()
        })
        .expect("no count-in");
    let mut pairs = Vec::new();
    let mut found = Vec::new();
    for (left, right) in [assigned, other] {
        let before = rig.project();
        let mut source = rig.source(
            &input,
            RecordingAlignment {
                synchronize: true,
                drift_correction: true,
                offset_ms: 0.0,
                input_sample_rate: None,
            },
        );
        source.left = left;
        source.right = right;
        rig.session
            .recording_start(source, 0, None)
            .expect("the port pair records");
        record_for(&rig, seconds, Duration::from_secs_f64(seconds + 40.0));
        rig.session.recording_stop().expect("the take is kept");
        let take = rig.new_takes(&before).remove(0);
        let measurement = stimulus.locate(&take.channel(0), options.min_score);
        let peak = take
            .audio
            .samples()
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        pairs.push(json!({
            "ports": [Some(left + 1), right.map(|port| port + 1)],
            "peak": peak,
            "measurement": measurement.json(),
        }));
        found.push((measurement.hits.len(), measurement.missed, peak));
        rig.session.undo();
    }
    result["pairs"] = Value::Array(pairs);
    check(
        &mut checks,
        "the assigned ports carry every burst",
        found[0].0 > 2 && found[0].1 == 0,
    );
    check(
        &mut checks,
        "the other ports carry none of it: the assignment replaced the ordinary output",
        found[1].0 == 0 && found[1].2 < found[0].2 * 0.05,
    );
    finish(&options, "external-ports", result, checks);
}

/// Records while something outside takes the output device away, and
/// reports what the engine and the take do. The file
/// `device-loss-ready-<label>` appears in the evidence folder when the
/// device may be taken; give it back a few seconds later.
#[test]
#[ignore = "opens real audio devices; set WINDFALL_DEVICE_EVIDENCE"]
fn device_loss_recovery() {
    let options = options();
    let input = required_input(&options);
    let rig = DeviceRig::open(&options);
    let (stimulus, _, _) = rig.add_stimulus(&options, 4.0);
    let mut result = header(&rig, &options, "device-loss");
    let mut checks = Vec::new();
    let before = rig.project();
    rig.session
        .transport_set(TransportPatch {
            count_in_bars: Some(0),
            ..TransportPatch::default()
        })
        .expect("no count-in");
    let source = rig.source(
        &input,
        RecordingAlignment {
            synchronize: true,
            drift_correction: true,
            offset_ms: 0.0,
            input_sample_rate: None,
        },
    );
    rig.session
        .recording_start(source, 0, None)
        .expect("recording starts before the device is lost");
    let ready = options
        .evidence
        .join(format!("device-loss-ready-{}", options.label));
    fs::write(&ready, "take the device away now\n").expect("the ready file is written");

    let began = Instant::now();
    let mut timeline = Vec::new();
    let mut was_running = true;
    let mut lost_at = None;
    let mut back_at = None;
    let mut take_error = None;
    let mut last_error = None;
    while began.elapsed() < Duration::from_secs_f64(options.loss_seconds) {
        std::thread::sleep(Duration::from_millis(50));
        rig.session.poll_engine_status();
        let status = rig.status();
        let state = rig.session.recording_state();
        if take_error.is_none() && state.error.is_some() {
            take_error = Some((began.elapsed(), state.error.clone()));
        }
        if status.running != was_running || status.error != last_error {
            timeline.push(json!({
                "atMs": began.elapsed().as_millis() as u64,
                "running": status.running,
                "error": status.error,
                "sampleRate": status.sample_rate,
                "bufferFrames": status.buffer_frames,
                "device": status.device,
            }));
            if was_running && !status.running && lost_at.is_none() {
                lost_at = Some(began.elapsed());
            }
            if !was_running && status.running && lost_at.is_some() && back_at.is_none() {
                back_at = Some(began.elapsed());
            }
            was_running = status.running;
            last_error = status.error.clone();
        }
        if back_at.is_some_and(|back| began.elapsed() > back + Duration::from_secs(2)) {
            break;
        }
    }
    let _ = fs::remove_file(&ready);
    result["timeline"] = Value::Array(timeline);
    result["lostAtMs"] = json!(lost_at.map(|at| at.as_millis() as u64));
    result["recoveredAtMs"] = json!(back_at.map(|at| at.as_millis() as u64));
    result["outageMs"] = json!(
        lost_at
            .zip(back_at)
            .map(|(lost, back)| (back - lost).as_millis() as u64)
    );
    result["takeErrorAtMs"] = json!(take_error.as_ref().map(|(at, _)| at.as_millis() as u64));
    result["takeError"] = json!(take_error.as_ref().and_then(|(_, error)| error.clone()));
    check(
        &mut checks,
        "the loss of the device is noticed",
        lost_at.is_some(),
    );
    check(
        &mut checks,
        "the output is reopened once the device is back",
        back_at.is_some() && rig.status().running,
    );
    // The take lost its clock with the stream. It must end as an error
    // that leaves the project and the recordings folder as they were.
    let stopped = rig.session.recording_stop();
    result["stopResult"] = json!(stopped.as_ref().err());
    check(
        &mut checks,
        "the interrupted take is refused, not kept as a broken clip",
        stopped.is_err() && rig.project().playlist.clips == before.playlist.clips,
    );
    rig.session.recording_cancel();
    let leftovers = fs::read_dir(rig.session.store().recordings_dir())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .count()
        })
        .unwrap_or(0);
    result["recordingFilesLeft"] = json!(leftovers);
    check(
        &mut checks,
        "no file of the interrupted take is left behind",
        leftovers == 0,
    );
    check(
        &mut checks,
        "recording is no longer active",
        !rig.session.recording_state().active,
    );
    if rig.status().running {
        let (again, measured, _) = synchronized_take(&rig, &options, &stimulus, &input, 0.0, 0);
        result["takeAfterRecovery"] = again;
        check(
            &mut checks,
            "a new take on the reopened device holds every burst",
            measured.is_some_and(|m| m.missed == 0 && m.hits.len() > 2),
        );
    }
    result["xrunsTotal"] = json!(rig.engine.controller().stream_stats().xruns);
    finish(&options, "device-loss", result, checks);
}

#[test]
fn the_burst_locator_finds_a_shifted_and_drifting_train_exactly() {
    // The measurement itself, checked on audio made by hand: a train that
    // starts 37 frames late and runs 100 ppm slow.
    let stimulus = Stimulus::new(48_000, 6.0, 0.05);
    let mut recorded = vec![0.0_f32; 48_000 * 6 + 4_800];
    for start in &stimulus.starts {
        let late = 37.0 + *start as f64 * 100e-6;
        let at = *start + late.round() as usize;
        for (index, sample) in stimulus.burst.iter().enumerate() {
            recorded[at + index] += sample * 0.02;
        }
    }
    let measured = stimulus.locate(&recorded, 0.5);
    assert_eq!(measured.missed, 0);
    assert_eq!(measured.hits.len(), stimulus.starts.len());
    assert!(
        (measured.mean_frames - 51.0).abs() < 2.0,
        "{}",
        measured.mean_frames
    );
    assert!(
        (measured.drift_ppm - 100.0).abs() < 12.0,
        "{}",
        measured.drift_ppm
    );
    assert!(measured.jitter_frames < 0.5, "{}", measured.jitter_frames);

    // Silence holds no burst, and says so instead of reporting an offset.
    let silent = stimulus.locate(&vec![0.0; recorded.len()], 0.5);
    assert_eq!(silent.hits.len(), 0);
    assert_eq!(silent.missed, stimulus.starts.len());
}
