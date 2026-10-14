//! Channel voice conversion from public PyFLP/DawVert tables and time fits.
use super::Builder;
use crate::{
    model::Channel,
    report::{Outcome, ReportSection},
};
use windfall_project::{
    ArpeggiatorMode, ChannelId, ChannelVoiceSettings, Command, EchoTime, NoteDivision,
};

impl Builder<'_> {
    pub(super) fn channel_sample_fx(
        &mut self,
        id: ChannelId,
        source: &Channel,
        target: windfall_project::TrackId,
        name: &str,
    ) {
        let p = source.params.unwrap_or_default();
        let mut effects = Vec::new();
        if p.remove_dc == Some(true) {
            effects.push((windfall_dsp::EffectKind::DcBlock, Vec::new()));
        }
        if source.fx_flags.is_some_and(|f| f & 256 != 0) {
            effects.push((
                windfall_dsp::EffectKind::StereoMatrix,
                vec![("ll", 0.0), ("rr", 0.0), ("lr", 1.0), ("rl", 1.0)],
            ));
        }
        if p.inverted == Some(true) {
            effects.push((
                windfall_dsp::EffectKind::Polarity,
                vec![("left", 1.0), ("right", 1.0)],
            ));
        }
        if !effects.is_empty() {
            let track = if self.channel_filters.contains_key(&source.iid) {
                self.project().channel(id).map_or(target, |c| c.mixer_track)
            } else {
                let Some(track) = self.create(
                    ReportSection::Channels,
                    "Sample effects bus",
                    Command::AddMixerTrack {
                        name: Some(format!("{name} sample effects")),
                    },
                ) else {
                    return;
                };
                let track = windfall_project::TrackId(track);
                self.apply(
                    ReportSection::Channels,
                    "Sample effects routing",
                    Command::SetTrackOutput {
                        id: track,
                        output: Some(target),
                    },
                );
                self.apply(
                    ReportSection::Channels,
                    "Sample effects owner",
                    Command::UpdateChannel {
                        id,
                        patch: windfall_project::ChannelPatch {
                            mixer_track: Some(track),
                            ..Default::default()
                        },
                    },
                );
                track
            };
            for (kind, values) in effects {
                let Some(effect) = self.create(
                    ReportSection::Channels,
                    "Sample effect",
                    Command::AddEffect {
                        track,
                        kind,
                        index: None,
                    },
                ) else {
                    continue;
                };
                let mut params = kind.default_params();
                for (id, value) in values {
                    if let Some(i) = kind.descriptors().iter().position(|d| d.id == id) {
                        params.set(i, value);
                    }
                }
                self.apply(
                    ReportSection::Channels,
                    "Sample effect controls",
                    Command::SetEffectParams {
                        track,
                        effect: windfall_project::EffectId(effect),
                        params,
                    },
                );
                self.report.say(ReportSection::Channels,Outcome::Approximated,format!("Channel {} sample {} mapped on its private bus; the stand-in processes summed voices instead of destructive source preprocessing.",source.iid,kind.name()));
            }
        }
        if let Some(p) = source.params {
            self.report.say(ReportSection::Channels,Outcome::Approximated,format!("Channel {} keyboard fine tune {:?} cents, main-pitch participation {:?}, inclusive key region {:?}, sample offset {:?}, loop crossfade {:?}/256 mapped; original controls retained.",source.iid,source.fine_tune,p.main_pitch,p.key_region,p.sample_offset,p.crossfade));
            for (field, value) in [
                (
                    "MIDI channel through",
                    p.midi_channel_through.filter(|v| *v != 0),
                ),
                (
                    "declick mode",
                    p.declick_mode.filter(|v| *v != 0).map(u32::from),
                ),
                ("fix legacy trim", p.fix_trim.filter(|v| *v).map(u32::from)),
            ] {
                if let Some(value) = value {
                    self.report.say(ReportSection::Channels,Outcome::Dropped,format!("SCOPE: Channel {} {field}={value}: PyFLP channel.py and DawVert channel.py establish the field/name; their converters and LMMS importer do not establish the MIDI-through operation, source declick kernel or legacy-trim dependency. This operation cannot be inferred from the serialized scalar; retained verbatim.",source.iid));
                }
            }
            for (i, v) in p.drum_stretch.into_iter().enumerate() {
                if let Some(v) = v.filter(|v| *v != 0.0 && *v != 1.0) {
                    self.report.say(ReportSection::Channels,Outcome::Dropped,format!("SCOPE: Channel {} drum stretch {}={v}: DawVert channel.py defines float fields tone/over/noise/band/time, but mi_flp.py, PyFLP and LMMS do not define their DSP units or kernel; original float retained.",source.iid,["tone","over","noise","band","time"][i]));
                }
            }
            if let Some(v) = p.stretch_formant.filter(|v| v.is_finite() && *v != 0.0) {
                self.report.say(ReportSection::Channels,Outcome::Dropped,format!("Channel {} stretch formant {v} semitones decoded with DawVert (word-.5)*24; omitted: the stand-in's spectral stretch has a boolean formant-preservation control rather than a formant offset.",source.iid));
                self.report.omit("Channel time stretch", &["formant"]);
            }
        }
    }
    /// Public mi_flp.py channel filter fit. A private bus preserves source
    /// mixer routing and keeps another channel's audio out of this filter.
    /// Processing after voice summation is an explicit stand-in approximation.
    pub(super) fn channel_filter(
        &mut self,
        id: ChannelId,
        channel: &Channel,
        target: windfall_project::TrackId,
        name: &str,
    ) {
        let automated=self.flp.initial_controls.iter().any(|v|matches!(crate::target::ControlTarget::decode(v.location),crate::target::ControlTarget::Channel {channel:iid,param:crate::target::ChannelParam::ModX|crate::target::ChannelParam::ModY} if iid==channel.iid))
            || self.flp.remote_controllers.iter().any(|v|matches!(v.target(),crate::target::ControlTarget::Channel {channel:iid,param:crate::target::ChannelParam::ModX|crate::target::ChannelParam::ModY} if iid==channel.iid))
            || self.flp.patterns.iter().any(|p|p.control_events.iter().any(|v|matches!(crate::target::ControlTarget::decode(v.location),crate::target::ControlTarget::Channel {channel:iid,param:crate::target::ChannelParam::ModX|crate::target::ChannelParam::ModY} if iid==channel.iid)));
        if channel.cutoff.is_none()
            && channel.resonance.is_none()
            && channel.filter_mod_x.is_none()
            && channel.filter_mod_y.is_none()
            && !automated
        {
            return;
        }
        let Some(track) = self.create(
            ReportSection::Channels,
            "Channel filter bus",
            Command::AddMixerTrack {
                name: Some(format!("{name} filter")),
            },
        ) else {
            return;
        };
        let track = windfall_project::TrackId(track);
        self.apply(
            ReportSection::Channels,
            "Channel filter routing",
            Command::SetTrackOutput {
                id: track,
                output: Some(target),
            },
        );
        self.apply(
            ReportSection::Channels,
            "Channel filter owner",
            Command::UpdateChannel {
                id,
                patch: windfall_project::ChannelPatch {
                    mixer_track: Some(track),
                    ..Default::default()
                },
            },
        );
        let kind = windfall_dsp::EffectKind::SelectableFilter;
        let Some(effect) = self.create(
            ReportSection::Channels,
            "Channel filter",
            Command::AddEffect {
                track,
                kind,
                index: None,
            },
        ) else {
            return;
        };
        let effect = windfall_project::EffectId(effect);
        let mut params = kind.default_params();
        let alternate = channel.filter_type.unwrap_or(0) != 0;
        let cutoff: fn(f64) -> f64 = if alternate {
            channel_cutoff_alt
        } else {
            channel_cutoff
        };
        let x = channel
            .filter_mod_x
            .map(f64::from)
            .unwrap_or_else(|| channel.cutoff.map_or(256.0, |v| f64::from(v) / 4.0));
        let y = channel.filter_mod_y.map(f64::from).unwrap_or_else(|| {
            channel
                .resonance
                .map_or(0.0, |v| f64::from(v) * 256.0 / 640.0)
        });
        for (name, value) in [
            ("frequencyHz", cutoff(x) as f32),
            ("q", channel_resonance(y) as f32),
            (
                "mode",
                match channel.filter_type.unwrap_or(0) {
                    2 => 2.0,
                    3 => 1.0,
                    4 => 3.0,
                    _ => 0.0,
                },
            ),
        ] {
            if let Some(i) = kind.descriptors().iter().position(|p| p.id == name) {
                params.set(i, value);
            }
        }
        self.apply(
            ReportSection::Channels,
            "Channel filter controls",
            Command::SetEffectParams {
                track,
                effect,
                params,
            },
        );
        self.channel_filters.insert(
            channel.iid,
            super::ImportedEffect {
                track,
                id: effect,
                kind,
                links: vec![
                    super::effects::ParamLink {
                        fl_param: 2,
                        fl_range: (0.0, 256.0),
                        id: "frequencyHz",
                        convert: cutoff,
                        destination_range: None,
                    },
                    super::effects::ParamLink {
                        fl_param: 3,
                        fl_range: (0.0, 256.0),
                        id: "q",
                        convert: channel_resonance,
                        destination_range: None,
                    },
                ],
            },
        );
        self.report.say(ReportSection::Channels,Outcome::Approximated,format!("Channel {} cutoff/resonance/type mapped on a private bus: modern /256, legacy cutoff /1024 and resonance /640; DawVert midi_filter=(n*100)^2*925/2048 (non-fast types *2.7), Q=1+6*n. Full cutoff uses a 20kHz bypass approximation. Double/SVF slopes and per-voice processing differ. Mod X/Y clip and recorded automation use the same functions.",channel.iid));
    }
    /// Runs after channel creation. The command replaces the whole voice
    /// block, so tools not described by Polyphony retain their defaults.
    pub(super) fn channel_voice(&mut self, id: ChannelId, channel: &Channel) -> Outcome {
        let section = ReportSection::Channels;
        let mut settings = ChannelVoiceSettings::default();
        let mut outcome = Outcome::Exact;
        if let Some(plugin) = channel
            .plugin
            .as_ref()
            .filter(|p| p.internal_name.eq_ignore_ascii_case("Fruity DX10"))
            && plugin.state.len() >= 92
        {
            let word = |i: usize| {
                u32::from_le_bytes(
                    plugin.state[i * 4..i * 4 + 4]
                        .try_into()
                        .expect("bounded DX10 state"),
                )
            };
            settings.envelopes.lfos.pitch = windfall_project::TargetLfo {
                enabled: word(11) != 0,
                shape: windfall_project::LfoShape::Sine,
                rate_hz: (word(14) as f32 / 65536.0 * 127.0).clamp(0.01, 20.0),
                depth: (word(11) as f32 / 65536.0 / 5.5).clamp(0.0, 1.0),
                ..Default::default()
            };
            self.report.say(section,Outcome::Approximated,format!("Channel {} Fruity DX10 vibrato/lforate mapped using DawVert amount=n/5.5, rate=n*127; shared pitch LFO depth and 0.01..20Hz stand-in range apply.",channel.iid));
            outcome = Outcome::Approximated;
        }
        let wrapper = channel.plugin.as_ref().and_then(|p| p.hosted());
        if let Some(range) = channel
            .pitch_range
            .map(u32::from)
            .or_else(|| wrapper.as_ref().and_then(|w| w.pitch_bend_range))
        {
            let mut map = self
                .project()
                .channel(id)
                .map_or_else(windfall_project::ExpressionMap::default, |c| {
                    c.expression_map
                });
            map.bend_range_semitones = (range as f32).clamp(1.0, 96.0);
            self.apply(
                section,
                "Channel pitch-bend range",
                Command::SetChannelExpressionMap {
                    id,
                    map,
                    expected: None,
                },
            );
            self.report.say(section,if (1..=96).contains(&range){Outcome::Exact}else{Outcome::Approximated},format!("Channel {} wrapper MIDI pitch range {range} semitones mapped to the existing plugin MIDI bend range (1..96); send-RPN flag {:?} retained. Event 83 is resonance, not pitch range; event 219 pitch is cents.",channel.iid,wrapper.as_ref().and_then(|w|w.send_pitch_bend_range)));
        }
        if let Some(plugin) = channel
            .plugin
            .as_ref()
            .filter(|p| p.internal_name.trim().eq_ignore_ascii_case("SimSynth"))
        {
            let state = &plugin.state;
            let v = |i: usize| {
                state
                    .get(i * 8..i * 8 + 8)
                    .and_then(|b| b.try_into().ok())
                    .map(f64::from_le_bytes)
                    .filter(|v| v.is_finite())
            };
            let flag = |i: usize| {
                state
                    .get(436 + i * 4..440 + i * 4)
                    .and_then(|b| b.try_into().ok())
                    .map(u32::from_le_bytes)
                    .is_some_and(|v| v != 0)
            };
            if state.len() >= 436 {
                settings.envelopes.filter = windfall_project::ModulationEnvelope {
                    enabled: flag(14),
                    attack_ms: (4.0 * v(41).unwrap_or(0.0).powi(3) * 1000.0).clamp(0.0, 60000.0)
                        as f32,
                    decay_ms: (4.0 * v(42).unwrap_or(0.0).powi(3) * 1000.0).clamp(0.0, 60000.0)
                        as f32,
                    sustain: v(43).unwrap_or(1.0).clamp(0.0, 1.0) as f32,
                    release_ms: (4.0 * v(44).unwrap_or(0.0).powi(3) * 1000.0).clamp(0.0, 60000.0)
                        as f32,
                    // DawVert's Vital modulation amount is env*.6 of its
                    // 128-semitone cutoff span: 6.4 octaves at full amount.
                    depth: (v(28).unwrap_or(0.0) * 6.4).clamp(-8.0, 8.0) as f32,
                };
                let source_shape = (v(24).unwrap_or(0.0) * 10.0).round() as i32;
                let shape = match source_shape {
                    0 => windfall_project::LfoShape::Triangle,
                    1 => windfall_project::LfoShape::Triangle,
                    2 => windfall_project::LfoShape::Square,
                    _ => windfall_project::LfoShape::Sine,
                };
                let multiplier = if matches!(source_shape, 1 | 4) {
                    2.0
                } else {
                    1.0
                };
                let rate = (2.0_f64.powf((1.0 + v(22).unwrap_or(0.5) * 998.0 - 500.0) / 100.0)
                    * multiplier)
                    .clamp(0.01, 20.0) as f32;
                // The public converter reads lfo_del and lfo_retrigger but
                // gives the delay no rule of its own; it uses the time rule
                // the same device publishes for its envelopes, 4*v^3 seconds.
                let delay_ms = (4.0 * v(21).unwrap_or(0.0).powi(3) * 1000.0)
                    .clamp(0.0, f64::from(windfall_project::TargetLfo::MAX_DELAY_MS))
                    as f32;
                let retrigger = flag(13);
                settings.envelopes.lfos.cutoff = windfall_project::TargetLfo {
                    enabled: flag(12) && flag(14),
                    shape,
                    rate_hz: rate,
                    depth: ((v(29).unwrap_or(0.5) - 0.5) * 2.0).clamp(-1.0, 1.0) as f32,
                    delay_ms,
                    retrigger,
                };
                let weighted = |offset: usize| {
                    let mut weight = 0.0;
                    let mut amount = 0.0;
                    for i in 0..3 {
                        if flag(i * 4) {
                            let level = v(i * 7 + 3).unwrap_or(0.0).clamp(0.0, 1.0);
                            weight += level;
                            amount += level * v(i * 7 + offset).unwrap_or(0.5);
                        }
                    }
                    if weight > 0.0 { amount / weight } else { 0.5 }
                };
                let mut pitch_env = settings.envelopes.filter;
                pitch_env.enabled = (weighted(5) - 0.5).abs() > f64::EPSILON;
                pitch_env.depth = ((weighted(5) - 0.5) * 4800.0).clamp(-2400.0, 2400.0) as f32;
                settings.envelopes.pitch = pitch_env;
                settings.envelopes.lfos.pitch = windfall_project::TargetLfo {
                    enabled: flag(12),
                    shape,
                    rate_hz: rate,
                    depth: ((weighted(4) - 0.5) * 2.0).clamp(-1.0, 1.0) as f32,
                    delay_ms,
                    retrigger,
                };
                self.report.say(section,Outcome::Approximated,format!("Channel {} SimSynth osc1/2/3_env and osc1/2/3_lfo are combined by enabled oscillator level into the stand-in's shared pitch routes; centered source amounts use (v-.5)*4800 cents and (v-.5)*2 LFO depth. Distinct oscillator trajectories cannot be represented by this stand-in.",channel.iid));
                self.report.say(section,Outcome::Approximated,format!("Channel {} SimSynth svf ADSR: 4*v^3 seconds, depth=env*6.4 octaves; LFO rate=2^((1+rate*998-500)/100), saw/noise doubled; on/cutoff depth mapped, source shape {source_shape} approximated by the available sine/triangle/square. lfo_del={:?} -> LFO start delay {delay_ms} ms (4*v^3 seconds, the device's published envelope time rule; the public converter reads the field without a rule of its own); lfo_retrigger -> retrigger={retrigger}. Oscillator-specific pitch routes have no matching per-oscillator stand-in controls.",channel.iid,v(21)));
                outcome = Outcome::Approximated;
            }
        }
        if let Some(polyphony) = channel.polyphony {
            settings.polyphony.max_voices = match polyphony.max {
                0 => {
                    outcome = Outcome::Approximated;
                    self.report.say(
                    section,
                    outcome,
                    "Unlimited FL channel polyphony was brought to Windfall's maximum of 32 voices.",
                );
                    32
                }
                1..=32 => polyphony.max as u8,
                max => {
                    outcome = Outcome::Approximated;
                    self.report.say(
                    section,
                    outcome,
                    format!("FL channel polyphony of {max} voices was clamped to Windfall's maximum of 32."),
                );
                    32
                }
            };
            settings.polyphony.mono_legato = polyphony.flags & 1 != 0;
            if polyphony.flags & 2 != 0 {
                settings.polyphony.portamento_ms =
                    ((f64::from(polyphony.slide) / 1024.0).powi(4) * 0.25 * 60000.0
                        / self.tempo_bpm)
                        .clamp(0.0, 60000.0) as f32;
                outcome = Outcome::Approximated;
                self.report.say(
                section,
                outcome,
                "Channel portamento time uses DawVert's (slide/1024)^4 steps fit, converted at the source tempo and clamped to Windfall's range.",
            );
            }
        }
        if let Some(params) = channel.params {
            if let Some(direction) = params.arp_direction.filter(|d| *d != 0) {
                settings.arpeggiator.mode = match direction {
                    1 => ArpeggiatorMode::Up,
                    2 => ArpeggiatorMode::Down,
                    3 | 4 => ArpeggiatorMode::UpDown,
                    _ => {
                        self.report.say(section, Outcome::Dropped, format!("Channel {} arpeggiator direction {direction} has no stand-in mode.",channel.iid));
                        ArpeggiatorMode::Off
                    }
                };
                settings.arpeggiator.range_octaves =
                    params.arp_range.unwrap_or(1).clamp(1, 4) as u8;
                settings.arpeggiator.gate =
                    (params.arp_gate.unwrap_or(48) as f32 / 48.0).clamp(0.0, 1.0);
                let beats = (f64::from(params.arp_time.unwrap_or(1024)) / 1024.0).powi(4) * 0.25;
                settings.arpeggiator.rate = [
                    NoteDivision::Quarter,
                    NoteDivision::Eighth,
                    NoteDivision::Sixteenth,
                    NoteDivision::ThirtySecond,
                ]
                .into_iter()
                .min_by(|a, b| {
                    (a.beats() - beats)
                        .abs()
                        .total_cmp(&(b.beats() - beats).abs())
                })
                .unwrap();
                outcome = Outcome::Approximated;
                self.report.say(section,outcome,format!("Channel {} arpeggiator direction/range/gate mapped; time uses DawVert's integer fourth-power fit and nearest Windfall division (PyFLP's float reading is disputed).",channel.iid));
                let mut omitted = Vec::new();
                for (unsupported, value) in [
                    ("chord", params.arp_chord.filter(|v| *v != u32::MAX)),
                    ("repeat", params.arp_repeat.filter(|v| *v > 1)),
                    ("slide", params.arp_slide.filter(|v| *v).map(u32::from)),
                ] {
                    if let Some(value) = value {
                        self.report.say(
                            section,
                            Outcome::Dropped,
                            format!(
                                "Channel {} arpeggiator {unsupported}={value}: omitted, the channel arpeggiator has no equivalent.",
                                channel.iid
                            ),
                        );
                        omitted.push(unsupported);
                    }
                }
                self.report.omit("Channel arpeggiator", &omitted);
                if direction == 4 {
                    self.report.say(
                        section,
                        Outcome::Dropped,
                        "Sticky arpeggiator endpoint repetition has no equivalent.",
                    );
                }
            }
            if let Some(gate) = params.gate.filter(|g| ![0, 450, 1446, 1447].contains(g)) {
                outcome = Outcome::Approximated;
                self.report.say(section,Outcome::Dropped,format!("SCOPE: Channel {} gate={gate}: PyFLP Time.gate gives musical endpoints only; DawVert channel.py stores a u16 without a logarithmic curve, and mi_flp.py/LMMS provide no formula for interior words. Source word retained.",channel.iid));
            } else if params.gate.is_some_and(|g| [450, 1446].contains(&g)) {
                self.report.say(section,Outcome::Approximated,format!("Channel {} gate={} mapped at its public musical endpoint (450=0:03 source ticks,1446=4:00 quarter notes); full_porta bypasses gate when portamento is active.",channel.iid,params.gate.unwrap()));
            }
            let mut omitted = Vec::new();
            for (flag, name, field) in [(2, "ping-pong", "ping_pong"), (4, "fat mode", "fat_mode")]
            {
                if params.delay_flags.is_some_and(|v| v & flag != 0) {
                    self.report.say(section,Outcome::Dropped,format!("Channel {} note echo {name}: omitted, Windfall's note echo has no such control.",channel.iid));
                    outcome = Outcome::Approximated;
                    omitted.push(field);
                }
            }
            self.report.omit("Channel note echo", &omitted);
        }
        if let Some(echo) = channel.echo.filter(|e| e.feedback != 0 && e.repeats != 0) {
            settings.echo.enabled = true;
            settings.echo.feedback = (echo.feedback as f32 / 12800.0).clamp(0.0, 0.95);
            // PyFLP's PPQ dependence and DawVert's /48 steps reading agree at
            // 96 PPQ with two source ticks per canonical note tick.
            let ms = f64::from(echo.time) / f64::from(self.ppq) * 0.5 * 60000.0 / self.tempo_bpm;
            settings.echo.time = EchoTime::Milliseconds {
                ms: ms.clamp(1.0, 2000.0) as f32,
            };
            settings.echo.repeats = echo.repeats.clamp(1, 8) as u8;
            settings.echo.pitch_semitones =
                (echo.pitch_cents as f32 / 100.0).round().clamp(-24.0, 24.0) as i8;
            outcome = Outcome::Approximated;
            self.report.say(section,outcome,format!("Channel {} note echo feedback/time/repeats/pitch mapped (DawVert step units); Windfall limits and whole-semitone pitch apply.",channel.iid));
            if echo.pan != 0 {
                self.report.say(
                    section,
                    Outcome::Dropped,
                    format!(
                        "Channel {} echo pan={}: omitted, Windfall's note echo has no independent echo-pan control.",
                        channel.iid, echo.pan
                    ),
                );
                self.report.omit("Channel note echo", &["pan"]);
            }
        }
        // One use of each device per channel in the review's list.
        let mut envelope_omitted = Vec::new();
        let mut lfo_omitted = Vec::new();
        for (index, env) in channel.envelopes.iter().enumerate() {
            let target = match index {
                0 => "pan",
                1 => "volume",
                2 => "cutoff",
                3 => "resonance",
                4 => "pitch",
                _ => "unknown",
            };
            if env.enabled && index != 1 && index < 5 {
                let mapped = windfall_project::ModulationEnvelope {
                    enabled: true,
                    attack_ms: crate::units::envelope_ms(env.attack),
                    decay_ms: crate::units::envelope_ms(env.decay),
                    sustain: env.sustain.min(128) as f32 / 128.0,
                    release_ms: crate::units::envelope_ms(env.release),
                    depth: env.amount.clamp(-128, 128) as f32 / 128.0
                        * match index {
                            2 => 8.0,
                            4 => 2400.0,
                            _ => 1.0,
                        },
                };
                match index {
                    0 => settings.envelopes.pan = mapped,
                    2 => settings.envelopes.filter = mapped,
                    3 => settings.envelopes.resonance = mapped,
                    4 => settings.envelopes.pitch = mapped,
                    _ => {}
                }
                outcome = Outcome::Approximated;
                self.report.say(section,outcome,format!("Channel {} {target} ADSR and depth mapped with documented source range and DawVert envelope time fit.",channel.iid));
            }
            if env.enabled {
                for (name, field, value) in [
                    ("predelay", "predelay", env.predelay.saturating_sub(100)),
                    ("hold", "hold", env.hold.saturating_sub(100)),
                    (
                        "attack tension",
                        "attack_tension",
                        env.attack_tension.unwrap_or(0).unsigned_abs(),
                    ),
                    (
                        "decay tension",
                        "decay_tension",
                        env.decay_tension.unwrap_or(0).unsigned_abs(),
                    ),
                    (
                        "release tension",
                        "release_tension",
                        env.release_tension.unwrap_or(0).unsigned_abs(),
                    ),
                    ("tempo sync", "tempo_sync", env.flags & 1),
                ] {
                    if value != 0 {
                        outcome = Outcome::Approximated;
                        self.report.say(section,Outcome::Dropped,format!("Channel {} {target} envelope {name}={value}: omitted, the channel envelope has no corresponding ADSR feature.",channel.iid));
                        if !envelope_omitted.contains(&field) {
                            envelope_omitted.push(field);
                        }
                    }
                }
            }
            if env.lfo_amount.is_some_and(|a| a != 0) {
                // Only the published 50% = sixteen steps anchor has a lawful
                // time conversion; do not guess an exponential period curve.
                if env.lfo_speed == Some(32950) && index < 5 {
                    let lfo = windfall_project::TargetLfo {
                        enabled: true,
                        shape: match env.lfo_shape.unwrap_or(0) {
                            1 => windfall_project::LfoShape::Triangle,
                            2 => windfall_project::LfoShape::Square,
                            _ => windfall_project::LfoShape::Sine,
                        },
                        rate_hz: (self.tempo_bpm / 240.0) as f32,
                        depth: env.lfo_amount.unwrap_or(0).clamp(-128, 128) as f32 / 128.0,
                        // PyFLP _EnvLFOFlags.LFOPhaseRetrig, bit 5.
                        retrigger: env.flags & 32 != 0,
                        ..Default::default()
                    };
                    match index {
                        0 => settings.envelopes.lfos.pan = lfo,
                        1 => settings.envelopes.lfos.volume = lfo,
                        2 => settings.envelopes.lfos.cutoff = lfo,
                        3 => settings.envelopes.lfos.resonance = lfo,
                        4 => settings.envelopes.lfos.pitch = lfo,
                        _ => {}
                    }
                    self.report.say(section,Outcome::Approximated,format!("Channel {} {target} LFO shape/depth and published sixteen-step period mapped at import tempo.",channel.iid));
                } else {
                    self.report.say(section,Outcome::Dropped,format!("SCOPE: Channel {} {target} LFO speed {:?}: PyFLP LFO.speed and DawVert channel.py establish range 200..65536 and 32950=sixteen steps, but no nonlinear period curve; mi_flp.py's LFO conversion is commented out, LMMS only reads raw data. Interior periods cannot be inferred; archived.",channel.iid,env.lfo_speed));
                }
                for (name, field, value) in [
                    (
                        "predelay",
                        "predelay",
                        env.lfo_predelay.unwrap_or(100).saturating_sub(100),
                    ),
                    (
                        "attack",
                        "attack",
                        env.lfo_attack.unwrap_or(100).saturating_sub(100),
                    ),
                    ("tempo sync", "tempo_sync", env.flags & 2),
                ] {
                    if value != 0 {
                        self.report.say(section,Outcome::Dropped,format!("Channel {} {target} LFO {name}={value}: omitted, the channel LFO has no control that takes it.",channel.iid));
                        if !lfo_omitted.contains(&field) {
                            lfo_omitted.push(field);
                        }
                    }
                }
                outcome = Outcome::Approximated;
            }
        }
        self.report.omit("Channel envelope", &envelope_omitted);
        self.report.omit("Channel LFO", &lfo_omitted);
        if self
            .apply(
                section,
                "Channel voice settings",
                Command::SetChannelVoiceSettings { id, settings },
            )
            .is_none()
        {
            return Outcome::Dropped;
        }
        outcome
    }
}

fn channel_cutoff(raw: f64) -> f64 {
    if raw >= 256.0 {
        20000.0
    } else {
        (raw.clamp(0.0, 256.0) / 256.0 * 100.0).powi(2) * 925.0 / 2048.0
    }
}
fn channel_cutoff_alt(raw: f64) -> f64 {
    if raw >= 256.0 {
        20000.0
    } else {
        channel_cutoff(raw) * 2.7
    }
}
fn channel_resonance(raw: f64) -> f64 {
    1.0 + 6.0 * raw.clamp(0.0, 256.0) / 256.0
}

#[cfg(test)]
mod tests {
    use crate::{
        convert::{Conversion, ConvertOptions, convert},
        model::{Channel, ChannelKind, FlpProject, Polyphony},
        report::{Outcome, ReportSection},
    };
    use windfall_project::ChannelVoiceSettings;

    fn imported(polyphony: Option<Polyphony>) -> Conversion {
        let source = FlpProject {
            channels: vec![Channel {
                kind: ChannelKind::Sampler,
                polyphony,
                ..Default::default()
            }],
            ..Default::default()
        };
        let result = convert(&source, &ConvertOptions::default());
        assert_eq!(result.project.channels.len(), 1);
        result.project.check().expect("valid imported project");
        result
    }

    fn expected(max_voices: u8, mono_legato: bool) -> ChannelVoiceSettings {
        let mut settings = ChannelVoiceSettings::default();
        settings.polyphony.max_voices = max_voices;
        settings.polyphony.mono_legato = mono_legato;
        settings
    }

    #[test]
    fn mono_with_max_four_sets_channel_voice_settings() {
        let result = imported(Some(Polyphony {
            max: 4,
            flags: 1,
            ..Default::default()
        }));
        assert_eq!(result.project.channels[0].voice, expected(4, true));
        let report = result.report.category(ReportSection::Channels);
        assert_eq!(report.exact, 1);
        assert!(report.lines.is_empty());
    }

    #[test]
    fn unlimited_max_becomes_thirty_two() {
        let result = imported(Some(Polyphony::default()));
        assert_eq!(result.project.channels[0].voice, expected(32, false));
        let report = result.report.category(ReportSection::Channels);
        assert_eq!(report.approximated, 1);
        assert_eq!(report.total(), 1);
        assert!(report.lines.iter().any(|line| {
            line.outcome == Outcome::Approximated && line.text.contains("Unlimited")
        }));
    }

    #[test]
    fn missing_polyphony_keeps_defaults_without_a_report_line() {
        let result = imported(None);
        assert_eq!(
            result.project.channels[0].voice,
            ChannelVoiceSettings::default()
        );
        let report = result.report.category(ReportSection::Channels);
        assert_eq!(report.exact, 1);
        assert!(report.lines.is_empty());
    }

    #[test]
    fn portamento_uses_documented_dawvert_fourth_power_time_fit() {
        for flags in [2, 3] {
            let result = imported(Some(Polyphony {
                max: 4,
                slide: 12_345,
                flags,
            }));
            let mut voice = expected(4, flags & 1 != 0);
            voice.polyphony.portamento_ms = 60000.0;
            assert_eq!(result.project.channels[0].voice, voice);
            let report = result.report.category(ReportSection::Channels);
            assert_eq!(report.approximated, 1);
            assert_eq!(report.total(), 1);
            assert_eq!(report.lines.len(), 1);
            let line = &report.lines[0];
            assert_eq!(line.outcome, Outcome::Approximated);
            assert!(line.text.contains("DawVert"));
            assert!(line.text.contains("slide/1024"));
            assert!(line.text.contains("clamped"));
        }
    }

    #[test]
    fn bounded_max_copies_through_and_larger_values_clamp() {
        for max in (1..=33).chain([u32::MAX]) {
            let result = imported(Some(Polyphony {
                max,
                ..Default::default()
            }));
            assert_eq!(
                result.project.channels[0].voice,
                expected(max.min(32) as u8, false)
            );
            let report = result.report.category(ReportSection::Channels);
            assert_eq!(report.total(), 1);
            if max <= 32 {
                assert_eq!(report.exact, 1);
                assert!(report.lines.is_empty());
            } else {
                assert_eq!(report.approximated, 1);
                assert!(
                    report
                        .lines
                        .iter()
                        .any(|line| line.text.contains("clamped"))
                );
            }
        }
    }

    #[test]
    fn slide_without_portamento_does_not_enable_glide() {
        let result = imported(Some(Polyphony {
            max: 8,
            slide: u32::MAX,
            flags: 0,
        }));
        assert_eq!(result.project.channels[0].voice, expected(8, false));
        assert!(
            result
                .report
                .category(ReportSection::Channels)
                .lines
                .is_empty()
        );
    }
}
