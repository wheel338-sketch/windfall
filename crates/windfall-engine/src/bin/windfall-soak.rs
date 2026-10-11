//! Soak test: plays a drum pattern through the real output device and
//! reports dropouts.
//!
//! ```text
//! windfall-soak [--seconds N] [--buffer N] [--sample-rate N] [--host NAME]
//!               [--device NAME] [--silent] [--effects] [--song]
//!               [--list-devices]
//! ```
//!
//! With `--effects` the master gets a reverb, a compressor and a limiter,
//! and a synth plays a bass line next to the drums. With `--song`, which
//! takes the effects along, all of that is arranged on the playlist and
//! played as a song that loops: four bars of the beat, an audio clip that
//! comes in half way, and two automation clips, one that rides the bass
//! track's fader and one that sweeps the synth's filter.
//!
//! The process exits with 0 only if the stream ran the whole time without a
//! single xrun.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use windfall_core::{AudioBuffer, TICKS_PER_STEP, gain_to_db};
use windfall_engine::{Engine, SamplePool};
use windfall_ipc::{AudioSettings, PlayMode, TransportPatch};
use windfall_project::{
    Automation, AutomationId, AutomationPoint, AutomationTarget, Channel, ChannelId, ChannelSource,
    Clip, ClipContent, ClipId, DEFAULT_KEY, EffectId, EffectKind, EffectSlot, InstrumentKind, Lane,
    MixerTrack, Note, NoteId, PlaylistTrack, PlaylistTrackId, Project, SampleAsset, SampleId,
    SamplePath, SamplerSettings, TrackId, palette_color,
};

/// Rate the drum sounds are generated at. It differs from the usual 48 kHz
/// device rate on purpose, so the test also exercises the resampler.
const SOUND_RATE: u32 = 44_100;

struct Options {
    seconds: u64,
    buffer_frames: u32,
    sample_rate: Option<u32>,
    host: Option<String>,
    device: Option<String>,
    silent: bool,
    effects: bool,
    song: bool,
    list_devices: bool,
}

fn main() -> ExitCode {
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("windfall-soak: {message}");
            eprintln!(
                "usage: windfall-soak [--seconds N] [--buffer N] [--sample-rate N] \
                 [--host NAME] [--device NAME] [--silent] [--effects] [--song] [--list-devices]"
            );
            return ExitCode::from(2);
        }
    };
    if options.list_devices {
        list_devices();
        return ExitCode::SUCCESS;
    }
    if soak(&options) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn parse_options(mut arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        seconds: 600,
        buffer_frames: 128,
        sample_rate: None,
        host: None,
        device: None,
        silent: false,
        effects: false,
        song: false,
        list_devices: false,
    };
    while let Some(flag) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--seconds" => options.seconds = number(&flag, &value()?)?,
            "--buffer" => options.buffer_frames = number(&flag, &value()?)?,
            "--sample-rate" => options.sample_rate = Some(number(&flag, &value()?)?),
            "--host" => options.host = Some(value()?),
            "--device" => options.device = Some(value()?),
            "--silent" => options.silent = true,
            "--effects" => options.effects = true,
            "--song" => {
                options.effects = true;
                options.song = true;
            }
            "--list-devices" => options.list_devices = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} needs a whole number, got \"{value}\""))
}

fn list_devices() {
    let hosts = Engine::devices();
    if hosts.is_empty() {
        println!("no audio hosts found");
    }
    for host in hosts {
        let default = if host.is_default { " (default)" } else { "" };
        println!("{}{default}", host.name);
        for device in host.devices {
            let default = if device.is_default { " (default)" } else { "" };
            let buffers = match (device.min_buffer_frames, device.max_buffer_frames) {
                (Some(min), Some(max)) => format!("{min}-{max} frames"),
                _ => "buffer range unknown".to_owned(),
            };
            println!("  {}{default}", device.name);
            println!("    sample rates: {:?}", device.sample_rates);
            println!("    buffers: {buffers}");
        }
    }
}

/// Runs the soak and returns whether it passed.
fn soak(options: &Options) -> bool {
    let engine = Engine::start(&AudioSettings {
        output_channels: None,
        host: options.host.clone(),
        device: options.device.clone(),
        sample_rate: options.sample_rate,
        buffer_frames: Some(options.buffer_frames),
    });
    let status = engine.status();
    if !status.running {
        let reason = status.error.unwrap_or_else(|| "unknown error".to_owned());
        println!("could not open an output stream: {reason}");
        println!(
            "SOAK RESULT: fail xruns=0 max_cpu=0.0% buffer_frames=0 sample_rate={}",
            status.sample_rate
        );
        return false;
    }
    // What was asked for and what the driver gave are different facts; a
    // shared-mode driver with one fixed period ignores the request.
    println!(
        "requested: host={} device={} sample_rate={} buffer_frames={}",
        options.host.as_deref().unwrap_or("default"),
        options.device.as_deref().unwrap_or("default"),
        options
            .sample_rate
            .map_or_else(|| "default".to_owned(), |rate| rate.to_string()),
        options.buffer_frames,
    );
    println!(
        "actual: host={} device={} sample_rate={} buffer_frames={} channels={} buffer_latency_ms={:.2} os={} arch={}",
        status.host,
        status.device.as_deref().unwrap_or("unknown device"),
        status.sample_rate,
        status.buffer_frames,
        status.output_channels,
        status.latency_ms,
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    println!(
        "playing on {} / {} at {} Hz, {} frames requested, {} seconds{}{}",
        status.host,
        status.device.as_deref().unwrap_or("unknown device"),
        status.sample_rate,
        options.buffer_frames,
        options.seconds,
        match (options.song, options.effects) {
            (true, _) => ", as a song with effects, a synth, an audio clip and automation",
            (false, true) => ", with effects and a synth",
            (false, false) => "",
        },
        if options.silent { ", silent" } else { "" },
    );

    let controller = engine.controller();
    let (mut project, mut pool) = drum_project();
    if options.effects {
        let (bass, bass_track) = add_effects(&mut project);
        if options.song {
            add_song(&mut project, &mut pool, bass, bass_track);
            controller.set_transport(TransportPatch {
                mode: Some(PlayMode::Song),
                loop_song: Some(true),
                ..TransportPatch::default()
            });
        }
    }
    controller.set_project(&project, &pool);
    if options.effects {
        println!(
            "the effects put the output {} frames behind",
            controller.latency_frames()
        );
    }
    if options.silent {
        controller.set_output_gain(0.0);
        // The gain glides to zero over a few milliseconds. Playback waits
        // until it is there, so not even the first hit is heard.
        std::thread::sleep(Duration::from_millis(200));
    }
    controller.play();

    let started = Instant::now();
    let mut max_cpu = 0.0_f32;
    let mut frames_before = 0;
    let mut stalled = false;
    let mut stream_error = None;
    for second in 1..=options.seconds {
        let wake = started + Duration::from_secs(second);
        std::thread::sleep(wake.saturating_duration_since(Instant::now()));

        let frame = controller.frame();
        let stats = controller.stream_stats();
        max_cpu = max_cpu.max(stats.cpu_peak);
        let master_peak = frame.meters.iter().take(2).fold(0.0_f32, |a, b| a.max(*b));
        let automated = if options.song {
            format!("  automated={}", frame.automated.len())
        } else {
            String::new()
        };
        println!(
            "{second:>5}s  xruns={}  cpu avg={:.2}% max={:.2}%  voices={}  master peak={:.1} dB{automated}",
            stats.xruns,
            stats.cpu * 100.0,
            stats.cpu_peak * 100.0,
            frame.voices,
            gain_to_db(master_peak),
        );

        let status = engine.status();
        if !status.running {
            stream_error = Some(
                status
                    .error
                    .unwrap_or_else(|| "the stream stopped".to_owned()),
            );
            break;
        }
        // A stream that is open but no longer asks for audio has failed too.
        if stats.frames == frames_before {
            stalled = true;
            break;
        }
        frames_before = stats.frames;
    }

    let status = engine.status();
    let stats = controller.stream_stats();
    max_cpu = max_cpu.max(stats.cpu_peak);
    if let Some(error) = &stream_error {
        println!("the stream failed: {error}");
    }
    if stalled {
        println!("the device stopped asking for audio");
    }
    let passed = stream_error.is_none() && !stalled && stats.xruns == 0;
    println!(
        "SOAK RESULT: {} xruns={} max_cpu={:.2}% buffer_frames={} sample_rate={} requested_buffer_frames={} frames_played={}",
        if passed { "pass" } else { "fail" },
        stats.xruns,
        max_cpu * 100.0,
        status.buffer_frames,
        status.sample_rate,
        options.buffer_frames,
        stats.frames,
    );
    passed
}

/// A one-bar four-on-the-floor beat: kick on every beat, snare on beats two
/// and four, and closed hats on every eighth note.
fn drum_project() -> (Project, SamplePool) {
    let mut project = Project::new("Soak");
    // Off the round 120, so the once-a-second report does not always catch
    // the beat at the same point.
    project.settings.tempo_bpm = 128.0;
    let mut pool = SamplePool::new();
    let pattern = &mut project.patterns[0];
    let rows: [(&str, AudioBuffer, &[u32], f32); 3] = [
        ("Kick", kick(), &[0, 4, 8, 12], 0.5),
        ("Snare", snare(), &[4, 12], 0.4),
        ("Hat", hat(), &[0, 2, 4, 6, 8, 10, 12, 14], 0.25),
    ];
    for (index, (name, sound, steps, volume)) in rows.into_iter().enumerate() {
        let mut next_id = || {
            project.next_id += 1;
            project.next_id - 1
        };
        let sample = SampleId(next_id());
        let track = TrackId(next_id());
        let channel = ChannelId(next_id());
        pool.insert(sample, sound);
        project.samples.push(SampleAsset {
            id: sample,
            name: name.to_owned(),
            path: SamplePath::Factory(format!("soak/{}.wav", name.to_lowercase())),
        });
        project.mixer.tracks.push(MixerTrack {
            dock: windfall_project::MixerDock::default(),
            external_output: None,
            processing: windfall_dsp::TrackParams::default(),
            current: false,
            latency_offset_ms: 0.0,
            id: track,
            name: name.to_owned(),
            color: palette_color(index),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            output: Some(TrackId::MASTER),
            sidechains: Vec::new(),
            sends: Vec::new(),
            effects: Vec::new(),
            recording: None,
        });
        project.channels.push(Channel {
            id: channel,
            name: name.to_owned(),
            color: palette_color(index),
            volume,
            pan: 0.0,
            muted: false,
            solo: false,
            group: String::new(),
            voice: Default::default(),
            expression_map: Default::default(),
            timing: windfall_project::ChannelTiming::default(),
            mixer_track: track,
            source: ChannelSource::Sampler(SamplerSettings {
                sample: Some(sample),
                // A new hat chokes the one before it, as on a real kit.
                cut_self: name == "Hat",
                ..SamplerSettings::default()
            }),
        });
        pattern.lanes.push(Lane {
            channel,
            notes: steps
                .iter()
                .map(|step| Note {
                    id: NoteId(next_id()),
                    start: step * TICKS_PER_STEP,
                    length: TICKS_PER_STEP,
                    key: DEFAULT_KEY,
                    velocity: 1.0,
                    pan: 0.0,
                    expression: Default::default(),
                })
                .collect(),
        });
    }
    (project, pool)
}

/// Gives the engine more to do: a reverb, a compressor and a limiter on the
/// master, each at its default settings, and a synth on a track of its own
/// that plays a bass line of eighth notes. Returns the synth's channel and
/// its mixer track.
fn add_effects(project: &mut Project) -> (ChannelId, TrackId) {
    let mut next_id = || {
        project.next_id += 1;
        project.next_id - 1
    };
    let chain = [
        EffectKind::Reverb,
        EffectKind::Compressor,
        EffectKind::Limiter,
    ];
    let effects: Vec<EffectSlot> = chain
        .into_iter()
        .map(|kind| EffectSlot {
            id: EffectId(next_id()),
            enabled: true,
            mix: 1.0,
            params: kind.default_params(),
        })
        .collect();
    let track = TrackId(next_id());
    let channel = ChannelId(next_id());
    let line = [36, 36, 48, 36, 39, 39, 51, 43];
    let notes = line
        .into_iter()
        .zip(0..)
        .map(|(key, step)| Note {
            id: NoteId(next_id()),
            start: step * 2 * TICKS_PER_STEP,
            length: TICKS_PER_STEP,
            key,
            velocity: 0.9,
            pan: 0.0,
            expression: Default::default(),
        })
        .collect();

    project.mixer.tracks[0].effects = effects;
    project.mixer.tracks.push(MixerTrack {
        dock: windfall_project::MixerDock::default(),
        external_output: None,
        processing: windfall_dsp::TrackParams::default(),
        current: false,
        latency_offset_ms: 0.0,
        id: track,
        name: "Bass".to_owned(),
        color: palette_color(3),
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        output: Some(TrackId::MASTER),
        sidechains: Vec::new(),
        sends: Vec::new(),
        effects: Vec::new(),
        recording: None,
    });
    project.channels.push(Channel {
        id: channel,
        name: "Bass".to_owned(),
        color: palette_color(3),
        volume: 0.8,
        pan: 0.0,
        muted: false,
        solo: false,
        group: String::new(),
        voice: Default::default(),
        expression_map: Default::default(),
        timing: windfall_project::ChannelTiming::default(),
        mixer_track: track,
        source: ChannelSource::Instrument {
                asset: None,
            params: InstrumentKind::SubtractiveSynth.default_params(),
        },
    });
    project.patterns[0].lanes.push(Lane { channel, notes });
    (channel, track)
}

/// Arranges the beat as a song of four bars: the pattern all the way
/// through, an audio clip of a riser over the last two bars on a mixer
/// track of its own, a ride on the bass track's fader, and a sweep of the
/// synth's filter up and back down.
fn add_song(project: &mut Project, pool: &mut SamplePool, bass: ChannelId, bass_track: TrackId) {
    let bar = project.settings.time_signature.ticks_per_bar();
    let song = bar * 4;
    let pattern = project.patterns[0].id;
    let mut next_id = || {
        project.next_id += 1;
        project.next_id - 1
    };
    let lanes: Vec<PlaylistTrackId> = (0..4).map(|_| PlaylistTrackId(next_id())).collect();
    let sample = SampleId(next_id());
    let riser_track = TrackId(next_id());
    let settings = InstrumentKind::SubtractiveSynth.descriptors();
    let cutoff = settings
        .iter()
        .position(|info| info.id == "filter.cutoffHz");
    let curves = [
        (
            AutomationTarget::TrackVolume { track: bass_track },
            [(0, 0.45), (bar * 2, 0.75), (song, 0.5)],
        ),
        (
            AutomationTarget::InstrumentParam {
                channel: bass,
                param: cutoff.unwrap_or(0) as u32,
            },
            [(0, 0.45), (bar * 3, 0.95), (song, 0.5)],
        ),
    ];
    let automations: Vec<Automation> = curves
        .into_iter()
        .enumerate()
        .map(|(index, (target, points))| Automation {
            id: AutomationId(next_id()),
            name: format!("Curve {}", index + 1),
            color: palette_color(index),
            target,
            points: points
                .into_iter()
                .map(|(tick, value)| AutomationPoint {
                    tick,
                    value,
                    curve: 0.3,
                    hold: false,
                })
                .collect(),
        })
        .collect();
    let contents = [
        (0, song, ClipContent::Pattern { pattern }),
        (
            bar * 2,
            bar * 2,
            ClipContent::Audio {
                sample,
                mixer_track: riser_track,
                output: Default::default(),
                normalize: false,
                gain: 0.5,
                pan: 0.0,
                fade_in: bar / 2,
                fade_out: bar / 4,
                reverse: false,
                pitch: 0.0,

                stretch: Default::default(),
            },
        ),
        (
            0,
            song,
            ClipContent::Automation {
                automation: automations[0].id,
            },
        ),
        (
            0,
            song,
            ClipContent::Automation {
                automation: automations[1].id,
            },
        ),
    ];
    let clips: Vec<Clip> = contents
        .into_iter()
        .zip(&lanes)
        .map(|((start, length, content), lane)| Clip {
            id: ClipId(next_id()),
            track: *lane,
            start,
            length,
            offset: 0,
            muted: false,
            content,
        })
        .collect();

    pool.insert(sample, riser());
    project.samples.push(SampleAsset {
        id: sample,
        name: "Riser".to_owned(),
        path: SamplePath::Factory("soak/riser.wav".to_owned()),
    });
    project.mixer.tracks.push(MixerTrack {
        dock: windfall_project::MixerDock::default(),
        external_output: None,
        processing: windfall_dsp::TrackParams::default(),
        current: false,
        latency_offset_ms: 0.0,
        id: riser_track,
        name: "Riser".to_owned(),
        color: palette_color(4),
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        output: Some(TrackId::MASTER),
        sidechains: Vec::new(),
        sends: Vec::new(),
        effects: Vec::new(),
        recording: None,
    });
    project.playlist.tracks = lanes
        .iter()
        .zip(["Beat", "Riser", "Bass level", "Bass filter"])
        .map(|(id, name)| PlaylistTrack {
            id: *id,
            name: name.to_owned(),
            muted: false,
            solo: false,
            color: 0,
            height: 0,
        })
        .collect();
    project.playlist.clips = clips;
    project.playlist.clips.sort_by_key(Clip::sort_key);
    project.automations = automations;
}

/// A sine that falls from 160 Hz to 45 Hz while it dies away.
fn kick() -> AudioBuffer {
    let mut phase = 0.0_f32;
    synthesize(0.35, |time| {
        let frequency = 45.0 + 115.0 * (-time * 28.0).exp();
        phase += frequency / SOUND_RATE as f32;
        (phase * std::f32::consts::TAU).sin() * (-time * 9.0).exp() * 0.9
    })
}

/// A burst of noise over a short 190 Hz body.
fn snare() -> AudioBuffer {
    let mut noise = Noise(0x1234_5678);
    synthesize(0.22, |time| {
        let body = (time * 190.0 * std::f32::consts::TAU).sin() * (-time * 30.0).exp();
        (noise.next() * (-time * 22.0).exp() * 0.7 + body * 0.4) * 0.8
    })
}

/// A very short burst of noise with the lows taken out.
fn hat() -> AudioBuffer {
    let mut noise = Noise(0x0BAD_5EED);
    let mut previous = 0.0;
    synthesize(0.06, |time| {
        let sample = noise.next();
        // The difference of neighbouring samples is a crude high-pass.
        let bright = (sample - previous) * 0.5;
        previous = sample;
        bright * (-time * 70.0).exp() * 0.6
    })
}

/// Four seconds of noise that gets louder and brighter as it goes.
fn riser() -> AudioBuffer {
    let mut noise = Noise(0x5EED_CAFE);
    let mut smoothed = 0.0;
    synthesize(4.0, |time| {
        let rise = time / 4.0;
        // A one-pole low-pass that lets more through as it rises.
        smoothed += (noise.next() - smoothed) * (0.02 + 0.6 * rise * rise);
        smoothed * rise * 0.6
    })
}

/// Builds a mono sound from a function of time in seconds, faded out over
/// its last few milliseconds so it ends on silence.
fn synthesize(seconds: f32, mut wave: impl FnMut(f32) -> f32) -> AudioBuffer {
    let frames = (seconds * SOUND_RATE as f32) as usize;
    let fade_frames = (0.005 * SOUND_RATE as f32) as usize;
    let data = (0..frames)
        .map(|frame| {
            let fade = ((frames - frame) as f32 / fade_frames as f32).min(1.0);
            wave(frame as f32 / SOUND_RATE as f32) * fade
        })
        .collect();
    AudioBuffer::from_interleaved(SOUND_RATE, 1, data)
}

/// White noise from a linear congruential generator, so every run of the
/// soak plays exactly the same sounds.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1 << 23) as f32 - 1.0
    }
}
