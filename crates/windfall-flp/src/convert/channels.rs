//! Channel conversion maps public sampler, keyboard and native device fields.
use super::{
    AudioSource, Builder, ChannelRole, PluginPlace, PluginPlaceholder, chosen_color, name_or, synth,
};
use crate::{
    model::{Channel, ChannelKind},
    paths::resolve,
    report::{Outcome, ReportSection},
    units,
};
use windfall_project::{
    ChannelId, ChannelPatch, Command, Envelope, SampleId, SamplePath, SamplerPatch,
};

impl Builder<'_> {
    pub(super) fn create(
        &mut self,
        section: ReportSection,
        what: &str,
        command: Command,
    ) -> Option<u32> {
        self.apply(section, what, command)?.first().copied()
    }

    fn sample(&mut self, _name: &str, path: Option<&str>) -> Option<SampleId> {
        let path = path.filter(|p| !p.trim().is_empty())?;
        let resolved = resolve(path, self.folders());
        let outcome = if resolved.resolved {
            Outcome::Exact
        } else {
            Outcome::Approximated
        };
        if !resolved.resolved {
            self.report.say(ReportSection::Samples, outcome, format!("The sample path \"{path}\" needs a folder supplied by the user; it was kept as written."));
        }
        let id = self.create(
            ReportSection::Samples,
            "A sample reference",
            Command::AddSample {
                name: crate::paths::file_stem(path).to_owned(),
                path: SamplePath::External(resolved.path),
            },
        );
        self.report.count(
            ReportSection::Samples,
            if id.is_some() {
                outcome
            } else {
                Outcome::Dropped
            },
            1,
        );
        id.map(SampleId)
    }

    fn preserve_clip_channel_notes(
        &mut self,
        channel: &Channel,
        name: &str,
        track: windfall_project::TrackId,
    ) -> bool {
        let iid = channel.iid;
        if !self.flp.patterns.iter().any(|p| {
            p.notes.iter().any(|n| n.channel == iid)
                || p.legacy_steps.iter().any(|s| s.channel == iid)
        }) {
            return false;
        }
        let id = self.create(
            ReportSection::Channels,
            "A note placeholder for a clip channel",
            Command::AddChannel {
                name: Some(name.to_owned()),
                sample: None,
                instrument: None,
                index: None,
                mixer_track: Some(track),
            },
        );
        if let Some(id) = id {
            self.channel_voice(ChannelId(id), channel);
            self.note_fallbacks.insert(iid, ChannelId(id));
            self.report.say(ReportSection::Channels, Outcome::Placeholder, "Notes addressed to an audio or automation clip channel were retained on a silent sampler; playlist clips still use their original content.");
            true
        } else {
            false
        }
    }

    pub(super) fn channels(&mut self) {
        for channel in &self.flp.channels {
            let name = name_or(channel.display_name(), || {
                format!("Channel {}", channel.iid)
            });
            let mixer_track = self.track_of(channel.insert);
            let volume = units::channel_gain(channel.volume.unwrap_or(10_240));
            let pan = units::channel_pan(channel.pan.unwrap_or(6_400));
            let master = if channel.params.is_none_or(|p| p.main_pitch != Some(false)) {
                self.flp.settings.main_pitch.unwrap_or(0)
            } else {
                0
            };
            let pitch = ((f64::from(channel.pitch.unwrap_or(0))
                + f64::from(channel.fine_tune.unwrap_or(0))
                + f64::from(master))
                / 100.0) as f32;
            match channel.kind {
                ChannelKind::Layer => {
                    self.channels.insert(
                        channel.iid,
                        ChannelRole::Layer(channel.layer_children.clone()),
                    );
                    self.report
                        .count(ReportSection::Channels, Outcome::Approximated, 1);
                    self.report.say(ReportSection::Channels, Outcome::Approximated, "Layer notes are copied to their children; layer controls have no equivalent.");
                    continue;
                }
                ChannelKind::AutomationClip => {
                    let notes = self.preserve_clip_channel_notes(channel, &name, mixer_track);
                    self.report.count(
                        ReportSection::Channels,
                        if notes {
                            Outcome::Placeholder
                        } else {
                            Outcome::Exact
                        },
                        1,
                    );
                    self.channels.insert(channel.iid, ChannelRole::Automation);
                    continue;
                }
                ChannelKind::AudioClip => {
                    let notes = self.preserve_clip_channel_notes(channel, &name, mixer_track);
                    let sample = self.sample(&name, channel.sample_path.as_deref());
                    let stretched = channel.params.is_some_and(|p| {
                        p.stretch_time.unwrap_or(0) != 0 || p.stretch_multiplier.unwrap_or(0) != 0
                    });
                    self.channels.insert(
                        channel.iid,
                        ChannelRole::Audio(AudioSource {
                            name,
                            sample,
                            mixer_track,
                            gain: volume,
                            pan,
                            reverse: channel.reversed(),
                            pitch,
                            stretched,
                            muted: !channel.enabled.unwrap_or(true),
                        }),
                    );
                    self.report.count(
                        ReportSection::Channels,
                        if notes {
                            Outcome::Placeholder
                        } else {
                            Outcome::Exact
                        },
                        1,
                    );
                    continue;
                }
                _ => {}
            }
            let translated = synth::translate(channel);
            if let Some(plugin) = &channel.plugin {
                for text in super::native::reports(&plugin.internal_name, &plugin.state) {
                    self.report
                        .say(ReportSection::Channels, Outcome::Approximated, text);
                }
            }
            let sampler = channel.kind == ChannelKind::Sampler && channel.plugin.is_none();
            let sample = if sampler {
                self.sample(&name, channel.sample_path.as_deref())
            } else {
                None
            };
            let instrument = translated.as_ref().map(|t| t.params.kind());
            let Some(id) = self.create(
                ReportSection::Channels,
                "A channel",
                Command::AddChannel {
                    name: Some(name.clone()),
                    sample,
                    instrument,
                    index: None,
                    mixer_track: Some(mixer_track),
                },
            ) else {
                continue;
            };
            let id = ChannelId(id);
            let voice_outcome = self.channel_voice(id, channel);
            self.channels.insert(channel.iid, ChannelRole::Plays(id));
            self.apply(
                ReportSection::Channels,
                "Channel controls",
                Command::UpdateChannel {
                    id,
                    patch: ChannelPatch {
                        color: chosen_color(channel.color),
                        group: channel
                            .group
                            .and_then(|g| usize::try_from(g).ok())
                            .and_then(|g| self.flp.channel_groups.get(g))
                            .map(|g| {
                                let mut name = g.trim().replace('\0', "");
                                let mut end = name
                                    .len()
                                    .min(windfall_project::MAX_CHANNEL_GROUP_NAME_BYTES);
                                while !name.is_char_boundary(end) {
                                    end -= 1;
                                }
                                name.truncate(end);
                                name
                            }),
                        timing: Some(channel_timing(channel, self.ppq)),
                        volume: Some(volume),
                        pan: Some(pan),
                        muted: Some(!channel.enabled.unwrap_or(true)),
                        ..Default::default()
                    },
                },
            );
            let mut outcome = Outcome::Exact;
            if let Some(group) = channel.group {
                self.report.say(ReportSection::Channels,Outcome::Approximated,format!("Channel {} source rack group {group} -> {:?}; target groups use trimmed display names, so duplicate source names share one target group and missing/negative references stay unassigned. Original group index retained.",channel.iid,self.project().channel(id).map(|c|c.group.as_str())));
            }
            self.channel_filter(id, channel, mixer_track, &name);
            self.channel_sample_fx(id, channel, mixer_track, &name);
            if channel
                .time_shift
                .is_some_and(|shift| shift != 0 && shift != 1024)
            {
                outcome = Outcome::Approximated;
                self.report.say(ReportSection::Channels, Outcome::Dropped, format!("SCOPE: Channel {} nonlinear time shift={:?}: PyFLP Time.shift provides endpoints 0/1024 but no curve; DawVert channel.py stores the word and mi_flp.py supplies no shift conversion; LMMS FlpImport.cpp only logs it. Interior tick values cannot be derived from endpoints; source word retained.",channel.iid,channel.time_shift));
            }
            if channel.time_shift == Some(1024) {
                self.report.say(ReportSection::Channels,Outcome::Approximated,format!("Channel {} time shift public 1024 endpoint (1:00) mapped to one quarter note; intermediate nonlinear curve is not inferred.",channel.iid));
            }
            if channel.swing.is_some_and(|swing| swing > 128) {
                outcome = Outcome::Approximated;
            }
            if let Some(translated) = translated {
                self.apply(
                    ReportSection::Channels,
                    "Instrument settings",
                    Command::SetInstrumentParams {
                        channel: id,
                        params: translated.params,
                    },
                );
                outcome = Outcome::Approximated;
                self.report.say(
                    ReportSection::Channels,
                    outcome,
                    format!(
                        "\"{name}\" became a Windfall instrument: {}.",
                        translated.notes.join("; ")
                    ),
                );
            } else if sampler {
                if channel.sampler_flags.is_some_and(|flags| flags & 8 != 0)
                    && let Some(sample) = sample
                {
                    self.sampler_loops.push((id, sample));
                }
                if channel.params.is_some_and(|p| p.normalize == Some(true))
                    && let Some(sample) = sample
                {
                    self.normalization.push((id, sample));
                    self.report.say(ReportSection::Samples,Outcome::Approximated,format!("Channel {} sample normalization resolves to inverse decoded peak times preamp, bounded by the sampler gain limit; missing audio needs a later relink.",channel.iid));
                }
                if let Some(threshold) = channel.params.and_then(|p| p.trim).filter(|v| *v != 0)
                    && let Some(sample) = sample
                {
                    self.sample_trim
                        .push((id, sample, threshold.min(256) as f32 / 256.0));
                    self.report.say(ReportSection::Samples,Outcome::Approximated,format!("Channel {} silence trim threshold={threshold}/256 mapped against decoded absolute PCM peak per frame; nonempty trim is intersected with the source window.",channel.iid));
                }
                let start = channel
                    .params
                    .and_then(|p| p.sample_start)
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.0)
                    + channel
                        .params
                        .and_then(|p| p.sample_offset)
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.0);
                let start = start.clamp(0.0, 0.999999) as f32;
                let length = channel
                    .params
                    .and_then(|p| p.sample_length)
                    .filter(|v| v.is_finite())
                    .unwrap_or(1.0);
                let end =
                    (f64::from(start) + length).clamp(f64::from(start) + 0.0000001, 1.0) as f32;
                if channel.params.is_some_and(|p| {
                    p.stretch_time.unwrap_or(0) != 0
                        || p.stretch_pitch.unwrap_or(0) != 0
                        || p.stretch_multiplier.unwrap_or(0) != 0
                }) || channel.sampler_flags.is_some_and(|f| f & 8 != 0)
                    || channel.fx_flags.is_some_and(|f| f & !2 != 0)
                    || channel.root_note.is_some_and(|k| k > 127)
                    || pitch.abs() > 48.0
                {
                    outcome = Outcome::Approximated;
                    self.report.say(ReportSection::Channels, outcome, "Sampler stretching, looping, extra sample effects or out-of-range tuning cannot be reproduced exactly.");
                }
                let cut = channel.cut.unwrap_or_default();
                self.apply(
                    ReportSection::Channels,
                    "Sampler settings",
                    Command::UpdateSampler {
                        id,
                        patch: SamplerPatch {
                            root_key: Some(channel.root_note.unwrap_or(60).min(127) as u8),
                            tune: Some(
                                pitch
                                    + channel.params.and_then(|p| p.stretch_pitch).unwrap_or(0)
                                        as f32
                                        / 100.0,
                            ),
                            stretch: sampler_stretch(channel),
                            gain: Some(f32::from(channel.preamp.unwrap_or(256)) / 256.0),
                            start: Some(start),
                            end: Some(end),
                            reverse: Some(channel.reversed()),
                            loop_crossfade: channel
                                .params
                                .and_then(|p| p.crossfade)
                                .map(|v| v.min(256) as f32 / 256.0),
                            cut_self: Some(cut.cuts != 0 && cut.cuts == cut.cut_by),
                            cut_group: Some(if cut.cuts == cut.cut_by {
                                cut.cuts.min(255) as u8
                            } else {
                                0
                            }),
                            ..SamplerPatch::default()
                        },
                    },
                );
                if cut.cuts != cut.cut_by || cut.cuts > 255 {
                    outcome = Outcome::Approximated;
                    self.report.say(
                        ReportSection::Channels,
                        outcome,
                        "Asymmetric or large cut groups cannot be represented exactly.",
                    );
                }
                if let Some(env) = channel.volume_envelope().filter(|e| e.enabled) {
                    outcome = Outcome::Approximated;
                    self.apply(
                        ReportSection::Channels,
                        "Sampler envelope",
                        Command::SetSamplerEnvelope {
                            id,
                            envelope: Some(Envelope {
                                attack_ms: units::envelope_ms(env.attack),
                                decay_ms: units::envelope_ms(env.decay),
                                sustain: env.sustain.min(128) as f32 / 128.0,
                                release_ms: units::envelope_ms(env.release),
                            }),
                        },
                    );
                    self.report.say(ReportSection::Channels, outcome, "The sampler volume ADSR uses fitted times; predelay, hold and tempo sync are reported separately, as are any unexpressible LFO values.");
                }
                if channel
                    .volume
                    .is_some_and(|v| !units::channel_gain_is_exact(v))
                {
                    outcome = Outcome::Approximated;
                    self.report.say(
                        ReportSection::Channels,
                        outcome,
                        "Channel volume uses DawVert's fitted gain curve.",
                    );
                }
            } else {
                outcome = Outcome::Placeholder;
                self.report.say(ReportSection::Channels, outcome, format!("\"{name}\" is a silent sampler placeholder; its notes and arrangement remain."));
                if let Some(plugin) = &channel.plugin {
                    let hosted = plugin.hosted();
                    self.plugins.push(PluginPlaceholder {
                        place: PluginPlace::Channel { channel: id },
                        internal_name: plugin.internal_name.clone(),
                        name: hosted.as_ref().and_then(|p| p.name.clone()),
                        vendor: hosted.as_ref().and_then(|p| p.vendor.clone()),
                        format: hosted.as_ref().map(|p| p.format),
                        path: hosted.as_ref().and_then(|p| p.path.clone()),
                        state: plugin.state.clone(),
                    });
                }
            }
            if outcome != Outcome::Placeholder
                && let Some(plugin) = &channel.plugin
            {
                self.plugins.push(PluginPlaceholder {
                    place: PluginPlace::Channel { channel: id },
                    internal_name: plugin.internal_name.clone(),
                    name: None,
                    vendor: None,
                    format: None,
                    path: None,
                    state: plugin.state.clone(),
                });
            }
            if let Some(plugin) = &channel.plugin
                && let Some(mut native) = super::slices::read(&plugin.internal_name, &plugin.state)
            {
                if plugin.internal_name.eq_ignore_ascii_case("Slicex")
                    && plugin.state.len() > 101
                    && native.playback.inline_source.is_none()
                {
                    self.report.say(ReportSection::Samples,Outcome::Approximated,"Slicex embedded WAV absent, malformed, compressed/unsupported or over the 4,194,304-frame PCM budget; filename is still resolved and the original payload retained. Public PCM8/16/24/32 and float32 formats are decoded; no compressed codec layout is inferred.");
                }
                let sample = self.sample(&name, native.path.as_deref());
                native.playback.sample = sample;
                if plugin.internal_name.eq_ignore_ascii_case("BassDrum")
                    && let Some(sample) = sample
                {
                    self.layer_samples.push((id, sample));
                    self.report.say(ReportSection::Samples,Outcome::Approximated,"BassDrum filename reference mapped to the Layer Kick PCM asset after decode; source sampling_ratio maps to sampleMix; mono/int16/32,768-frame stand-in limits apply.");
                }
                if native.cue_source
                    && let Some(sample) = sample
                {
                    self.sample_cues.push((id, sample));
                }
                if native.playback.inline_source.is_some() {
                    self.report.say(ReportSection::Samples,Outcome::Approximated,"Slicex embedded WAV PCM/cues decoded as mono fallback (stereo is averaged), bounded to 4,194,304 frames; original WAV bytes stay in the native blob. External sample takes precedence when resolved.");
                }
                let region = &native.playback.slices[0];
                if let Some(sample) = sample
                    && !native.slices.is_empty()
                {
                    self.native_slices.push(super::SliceRequest {
                        channel: id,
                        sample,
                        slices: native.slices,
                        pitch: region.pitch,
                        attack_ms: region.attack_ms,
                        release_ms: region.release_ms,
                        base_key: native.base_key,
                    });
                }
                let supported=self.project().channel(id).is_some_and(|c|matches!(c.source,windfall_project::ChannelSource::Instrument {params,..} if windfall_project::sample_playback::supported(params.kind())));
                if supported {
                    self.apply(
                        ReportSection::Channels,
                        "Native sample playback reference",
                        Command::SetSamplePlayback {
                            channel: id,
                            asset: Some(native.playback),
                        },
                    );
                }
                self.report.say(ReportSection::Channels,Outcome::Approximated,format!("{} native sample reference and pitch/slice metadata mapped; slice boundaries resolve against decoded sample frames using DawVert's channels/2 offset convention; attack/release use a disclosed 0..128 to 0..100ms fit.",plugin.internal_name));
            }
            if channel
                .plugin
                .as_ref()
                .is_some_and(|p| p.internal_name.eq_ignore_ascii_case("Fruity Granulizer"))
                && channel.sample_path.is_some()
            {
                let sample = self.sample(&name, channel.sample_path.as_deref());
                self.apply(
                    ReportSection::Samples,
                    "Granular native sample reference",
                    Command::SetSamplePlayback {
                        channel: id,
                        asset: Some(windfall_project::SamplePlayback {
                            sample,
                            ..Default::default()
                        }),
                    },
                );
                self.report.say(ReportSection::Samples,Outcome::Exact,"Granular generator channel event 196 sample reference mapped to its prepared sample-playback asset.");
            }
            self.report
                .count(ReportSection::Channels, outcome.max(voice_outcome), 1);
        }
    }
}

/// Only the documented speed multiplier is dimensionless. A requested time
/// needs source audio duration, so it cannot be inferred by this converter.
fn sampler_stretch(channel: &Channel) -> Option<windfall_project::SamplerStretch> {
    let params = channel.params?;
    if params.stretch_time.unwrap_or(0) != 0 || params.stretch_mode.unwrap_or(0) == 0 {
        return None;
    }
    let ratio = 2.0_f64.powf(-f64::from(params.stretch_multiplier.unwrap_or(0)) / 10_000.0);
    if !(0.25..=4.0).contains(&ratio) {
        return None;
    }
    Some(windfall_project::SamplerStretch::Spectral {
        ratio,
        quality: Default::default(),
        formants: false,
        range: windfall_project::SamplerKeyRange {
            first: 0,
            last: 127,
        },
    })
}

/// Only the published musical anchors are defined. The sources do not give a
/// formula for the nonlinear interior; interpolation would fabricate timing.
fn channel_timing(channel: &Channel, ppq: u16) -> windfall_project::ChannelTiming {
    let mut timing = windfall_project::ChannelTiming {
        swing_mix: channel.swing.map_or(1.0, |s| f32::from(s.min(128)) / 128.0),
        shift_ticks: if channel.time_shift == Some(1024) {
            windfall_project::PPQ as i32
        } else {
            0
        },
        ..Default::default()
    };
    let bypass = channel.params.is_some_and(|p| p.full_porta == Some(true))
        && channel.polyphony.is_some_and(|p| p.flags & 2 != 0);
    if !bypass {
        timing.gate_ticks = match channel.params.and_then(|p| p.gate) {
            Some(450) => crate::units::rescale(3, ppq).ticks as u32,
            Some(1446) => windfall_project::PPQ * 4,
            _ => 0,
        };
    }
    timing
}
