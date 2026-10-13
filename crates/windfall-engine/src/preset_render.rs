//! Bounded control-side rendering of Windfall's own instrument presets.
//! Playback and rendering share the engine, including PDC and prepared speech.
use crate::{RenderOptions, SamplePool, render_reporting};
use windfall_core::{AudioBuffer, PPQ};
use windfall_dsp::{InstrumentKind, InstrumentParams};
use windfall_ipc::PlayMode;
use windfall_project::*;

pub const MAX_PRESET_BYTES: usize = 1024 * 1024;
pub const MAX_RENDER_BYTES: usize = 128 * 1024 * 1024;
/// Explicit capability table; adding a kind requires measured acceptance.
pub const SYNTH_KINDS: &[InstrumentKind] = &[
    InstrumentKind::SubtractiveSynth,
    InstrumentKind::FourOp,
    InstrumentKind::MatrixFm,
    InstrumentKind::RingHybrid,
    InstrumentKind::HarmonicStack,
    InstrumentKind::PartialMorph,
    InstrumentKind::Inharmonic,
    InstrumentKind::Resynth,
    InstrumentKind::SeedPatch,
    InstrumentKind::ScanSynth,
    InstrumentKind::AcidLine,
    InstrumentKind::TripleOsc,
    InstrumentKind::WaveLane,
    InstrumentKind::MacroVoice,
    InstrumentKind::VintagePoly,
    InstrumentKind::DualPoly,
    InstrumentKind::CompactSynth,
    InstrumentKind::VintageSaw,
    InstrumentKind::PocketSynth,
    InstrumentKind::GrooveSynth,
    InstrumentKind::HybridSeq,
    InstrumentKind::MatrixSynth,
    InstrumentKind::PackSynth,
    InstrumentKind::SnapshotMorph,
    InstrumentKind::HarmonicFilter,
    InstrumentKind::MetalVoice,
    InstrumentKind::NumberVoice,
    InstrumentKind::SpectralResynth,
    InstrumentKind::ColorScan,
];

pub use windfall_project::PresetRenderSettings as PresetRenderOptions;
pub fn check_options(options: PresetRenderOptions, rate: u32) -> Result<(), String> {
    options.check()?;
    if !(8000..=192000).contains(&rate) {
        return Err("Preset rendering needs a project rate of 8–192 kHz.".into());
    }
    Ok(())
}

pub struct PreparedPreset {
    pub project: Project,
    pub pool: SamplePool,
    pub options: RenderOptions,
}

/// Public for the measured live/offline test and auditioning the same recipe.
pub fn prepare(
    bytes: &[u8],
    options: PresetRenderOptions,
    rate: u32,
    keep_going: &mut dyn FnMut() -> bool,
) -> Result<PreparedPreset, String> {
    check_options(options, rate)?;
    if bytes.len() > MAX_PRESET_BYTES {
        return Err("A renderable Windfall preset may be at most 1 MiB.".into());
    }
    if !keep_going() {
        return Err("Preset rendering cancelled".into());
    }
    let (preset, _) = Preset::from_json(bytes)?;
    let (params, asset, speech, volume, pan, voice, timing, expression_map) = if let Some(channel) =
        preset.channel
    {
        if channel.plugin.is_some() || channel.rack.is_some() {
            return Err("Render as sample accepts a built-in instrument preset without hosted plugins or a modular rack.".into());
        }
        let ChannelSource::Instrument { params, asset } = channel.source else {
            return Err("Choose a Windfall synthesizer or speech instrument preset.".into());
        };
        (
            params,
            asset,
            channel.sample_instrument,
            channel.volume,
            channel.pan,
            channel.voice,
            channel.timing,
            channel.expression_map,
        )
    } else if let Some(PluginPreset {
        state: PluginPresetState::Instrument { params, asset },
        ..
    }) = preset.plugin
    {
        (
            params,
            asset,
            None,
            1.0,
            0.0,
            Default::default(),
            Default::default(),
            Default::default(),
        )
    } else {
        return Err("Choose a Windfall synthesizer or speech instrument preset.".into());
    };
    let kind = params.kind();
    if !SYNTH_KINDS.contains(&kind)
        && !matches!(
            kind,
            InstrumentKind::SpeechVoice | InstrumentKind::TextVoice
        )
    {
        return Err(format!(
            "{} presets cannot be rendered as samples here.",
            kind.name()
        ));
    }
    let seconds = match (&params, &speech) {
        (InstrumentParams::SpeechVoice(voice), _) => {
            if voice.phrase.length == 0 {
                return Err(
                    "Speech preset phrase is empty. Enter phonemes before rendering.".into(),
                );
            }
            (f32::from(voice.phrase.length) / voice.rate).max(options.length_secs)
        }
        (
            InstrumentParams::TextVoice(voice),
            Some(SampleInstrumentAsset::Speech { duration, .. }),
        ) => {
            // Keep the complete spoken phrase, including transposed playback.
            (*duration * 2.0_f32.powf((60.0 - f32::from(options.note) - voice.tune) / 12.0))
                .max(options.length_secs)
        }
        (_, None) if kind == InstrumentKind::TextVoice => {
            return Err(
                "Speech preset text is empty. Save a Text Voice channel preset with a phrase."
                    .into(),
            );
        }
        _ => options.length_secs,
    };
    let frames = ((f64::from(seconds) + f64::from(options.tail_secs)) * f64::from(rate)).ceil();
    if seconds > 240.0 || frames * 8.0 > MAX_RENDER_BYTES as f64 {
        return Err("Preset render exceeds the 240 s / 128 MiB stereo PCM limit. Shorten the note or tail, or raise the speech note.".into());
    }
    let ticks = (f64::from(seconds) * f64::from(PPQ)).ceil() as u32;
    let mut doc = Document::new(Project::new("Preset render"));
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: Some(preset.name),
                sample: None,
                instrument: Some(kind),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .map_err(|e| e.to_string())?
        .created[0],
    );
    // The private render document has no effects or external plugin bindings.
    let mut project = doc.project().clone();
    project.settings.tempo_bpm = 60.0;
    let target = project
        .channels
        .iter_mut()
        .find(|c| c.id == channel)
        .expect("created channel");
    target.source = ChannelSource::Instrument { params, asset };
    target.sample_instrument = speech;
    target.volume = volume;
    target.pan = pan;
    target.voice = voice;
    target.timing = timing;
    target.expression_map = expression_map;
    let pattern = project.patterns[0].id;
    project.patterns[0].length_steps = ticks.div_ceil(PPQ / 4).max(1);
    let mut doc = Document::new(project);
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel,
            notes: vec![NoteInit {
                start: 0,
                length: ticks,
                key: options.note,
                velocity: Some(options.velocity),
                pan: None,
                expression: None,
            }],
        },
        None,
    )
    .map_err(|e| e.to_string())?;
    let track = PlaylistTrackId(
        doc.dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .map_err(|e| e.to_string())?
        .created[0],
    );
    doc.dispatch(
        Command::AddClips {
            clips: vec![ClipInit {
                track,
                start: 0,
                length: Some(ticks),
                offset: None,
                muted: None,
                content: ClipContent::Pattern { pattern },
            }],
        },
        None,
    )
    .map_err(|e| e.to_string())?;
    let project = doc.project().clone();
    project.check()?;
    let pool = SamplePool::new().prepare_sample_instruments(&project, keep_going)?;
    Ok(PreparedPreset {
        project,
        pool,
        options: RenderOptions {
            sample_rate: rate,
            mode: PlayMode::Song,
            tail_secs: options.tail_secs,
            auto_tail: true,
            block_frames: 256,
            ..Default::default()
        },
    })
}

pub fn render(
    bytes: &[u8],
    options: PresetRenderOptions,
    rate: u32,
    keep_going: &mut dyn FnMut() -> bool,
) -> Result<AudioBuffer, String> {
    let prepared = prepare(bytes, options, rate, keep_going)?;
    let mut cancelled = false;
    let rendered = render_reporting(
        &prepared.project,
        &prepared.pool,
        &prepared.options,
        &mut |_| {
            let proceed = keep_going();
            cancelled |= !proceed;
            proceed
        },
    );
    if cancelled || !keep_going() {
        return Err("Preset rendering cancelled".into());
    }
    if let Some(error) = rendered.timeline_error.or(rendered.plugin_error) {
        return Err(error);
    }
    if let Some(error) = rendered.sampler_error {
        return Err(error.to_string());
    }
    if rendered.audio.frames() == 0 || rendered.audio.samples().iter().any(|s| !s.is_finite()) {
        return Err("The preset produced no finite audio.".into());
    }
    if options.stereo {
        Ok(rendered.audio)
    } else {
        let mono = rendered
            .audio
            .samples()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| (s[0] + s[1]) * 0.5)
            .collect();
        Ok(AudioBuffer::from_interleaved(rate, 1, mono))
    }
}
