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
    /// Runs after channel creation. The command replaces the whole voice
    /// block, so tools not described by Polyphony retain their defaults.
    pub(super) fn channel_voice(&mut self, id: ChannelId, channel: &Channel) -> Outcome {
        let section = ReportSection::Channels;
        let mut settings = ChannelVoiceSettings::default();
        let mut outcome = Outcome::Exact;
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
                                "Channel {} arpeggiator {unsupported}={value} has no equivalent.",
                                channel.iid
                            ),
                        );
                    }
                }
                if direction == 4 {
                    self.report.say(
                        section,
                        Outcome::Dropped,
                        "Sticky arpeggiator endpoint repetition has no equivalent.",
                    );
                }
            }
            if let Some(gate) = params.gate.filter(|g| *g != 1447 && *g != 0) {
                outcome = Outcome::Approximated;
                self.report.say(section,Outcome::Dropped,format!("Channel {} gate={gate}: public sources give logarithmic endpoints but no tick conversion; kept in the source archive.",channel.iid));
            }
            for (flag, name) in [(2, "ping-pong"), (4, "fat mode")] {
                if params.delay_flags.is_some_and(|v| v & flag != 0) {
                    self.report.say(section,Outcome::Dropped,format!("Channel {} note echo {name} is not expressible by Windfall's note echo.",channel.iid));
                    outcome = Outcome::Approximated;
                }
            }
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
                        "Channel {} echo pan={} has no independent echo-pan control.",
                        channel.iid, echo.pan
                    ),
                );
            }
        }
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
                for (name, value) in [
                    ("predelay", env.predelay.saturating_sub(100)),
                    ("hold", env.hold.saturating_sub(100)),
                    (
                        "attack tension",
                        env.attack_tension.unwrap_or(0).unsigned_abs(),
                    ),
                    (
                        "decay tension",
                        env.decay_tension.unwrap_or(0).unsigned_abs(),
                    ),
                    (
                        "release tension",
                        env.release_tension.unwrap_or(0).unsigned_abs(),
                    ),
                    ("tempo sync", env.flags & 1),
                ] {
                    if value != 0 {
                        outcome = Outcome::Approximated;
                        self.report.say(section,Outcome::Dropped,format!("Channel {} {target} envelope {name}={value} has no corresponding ADSR feature.",channel.iid));
                    }
                }
            }
            if env.lfo_amount.is_some_and(|a| a != 0) {
                // Only the published 50% = sixteen steps anchor has a lawful
                // time conversion; do not guess an exponential period curve.
                if env.lfo_speed == Some(32950) && index != 1 && index < 5 {
                    let lfo = windfall_project::TargetLfo {
                        enabled: true,
                        shape: match env.lfo_shape.unwrap_or(0) {
                            1 => windfall_project::LfoShape::Triangle,
                            2 => windfall_project::LfoShape::Square,
                            _ => windfall_project::LfoShape::Sine,
                        },
                        rate_hz: (self.tempo_bpm / 240.0) as f32,
                        depth: env.lfo_amount.unwrap_or(0).clamp(-128, 128) as f32 / 128.0,
                    };
                    match index {
                        0 => settings.envelopes.lfos.pan = lfo,
                        2 => settings.envelopes.lfos.cutoff = lfo,
                        3 => settings.envelopes.lfos.resonance = lfo,
                        4 => settings.envelopes.lfos.pitch = lfo,
                        _ => {}
                    }
                    self.report.say(section,Outcome::Approximated,format!("Channel {} {target} LFO shape/depth and published sixteen-step period mapped at import tempo.",channel.iid));
                } else {
                    self.report.say(section,Outcome::Dropped,format!("Channel {} {target} LFO speed {:?}: no public period conversion for this raw value; archived.",channel.iid,env.lfo_speed));
                }
                for (name, value) in [
                    (
                        "predelay",
                        env.lfo_predelay.unwrap_or(100).saturating_sub(100),
                    ),
                    ("attack", env.lfo_attack.unwrap_or(100).saturating_sub(100)),
                    ("sync/retrigger", env.flags & 34),
                ] {
                    if value != 0 {
                        self.report.say(section,Outcome::Dropped,format!("Channel {} {target} LFO {name}={value} is not expressible by the stand-in.",channel.iid));
                    }
                }
                outcome = Outcome::Approximated;
            }
        }
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
