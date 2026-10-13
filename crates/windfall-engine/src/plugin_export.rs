//! Narrow callback entry points for an externally clocked engine. Preparation,
//! publication and disposal belong to the plugin owner's main thread.

use super::{Controller, Message, Processor};
use crate::SamplePool;
use windfall_ipc::PlayMode;
use windfall_project::{PatternId, Project};

impl Processor {
    /// Prepare a complete detached project before attaching the processor.
    /// The caller validates assets and routes. Must run off the audio thread.
    pub fn prepare_plugin(
        project: &Project,
        samples: &SamplePool,
        sample_rate: u32,
        output_channels: usize,
    ) -> Result<(Self, Controller), String> {
        let prepared =
            Controller::try_prepare_project(project, samples).map_err(|e| e.to_string())?;
        let controller = Controller::new();
        controller.set_prepared_project(project, prepared);
        if let Some(error) = controller.project_preparation_error() {
            return Err(error.to_string());
        }
        let (mut processor, _, retirement) = controller
            .try_attach(
                sample_rate,
                crate::project_preparation::AttachmentMode::Independent,
            )
            .map_err(|e| e.to_string())?;
        drop(retirement);
        processor.set_output_channels(output_channels);
        // Consume the numeric startup prefix before the external clock can
        // play. Otherwise the first callback's default pattern/seek messages
        // would overwrite the host's song-mode anchor.
        processor.handle_messages();
        processor.handle(Message::SetTransport {
            mode: PlayMode::Song,
            pattern: project.patterns.first().map_or(PatternId(0), |p| p.id),
            loop_song: false,
            metronome: Default::default(),
        });
        Ok((processor, controller))
    }

    /// Host beat position is in quarter notes. Negative preroll holds the
    /// engine stopped at zero; it never starts the first song note early.
    /// Internal tempo maps require a separate arbitration policy and are refused.
    /// No controller lock, allocation or plan publication occurs here.
    pub fn sync_plugin_transport(&mut self, beats: f64, tempo: f64, playing: bool) -> bool {
        if !beats.is_finite()
            || !tempo.is_finite()
            || !(windfall_project::MIN_TEMPO_BPM..=windfall_project::MAX_TEMPO_BPM).contains(&tempo)
            || self.plan.tempo_map.is_some()
        {
            return false;
        }
        let tick = beats.max(0.0) * f64::from(windfall_core::PPQ);
        if !tick.is_finite() || tick > f64::from(windfall_project::MAX_SONG_TICKS) {
            return false;
        }
        self.plugin_tempo = Some(tempo);
        self.sequencer.set_plugin_tempo(tempo, self.frame);
        let playing = playing && beats >= 0.0;
        if !playing && self.sequencer.playing() {
            self.handle(Message::Stop {
                sequence: self.transport_sequence.wrapping_add(1),
            });
        }
        if (self.sequencer.tick(&self.plan, self.frame) - tick).abs() > 1.0 {
            self.handle(Message::Seek(tick));
        }
        if playing && !self.sequencer.playing() {
            self.handle(Message::Play {
                sequence: self.transport_sequence.wrapping_add(1),
                passes: None,
                from: Some(tick),
            });
        }
        true
    }
    pub fn set_plugin_meter(&mut self, numerator: u16, denominator: u16) -> bool {
        if numerator == 0 && denominator == 0 {
            return true;
        } // host has no meter
        if numerator == 0
            || numerator > 32
            || denominator == 0
            || denominator > 32
            || !denominator.is_power_of_two()
        {
            return false;
        }
        self.plugin_meter = Some(windfall_project::TimeSignature {
            numerator: numerator as u8,
            denominator: denominator as u8,
        });
        true
    }
    pub fn plugin_meter(&self) -> (u16, u16) {
        let meter = self.plugin_meter.unwrap_or(self.plan.signature);
        (u16::from(meter.numerator), u16::from(meter.denominator))
    }

    /// Read the bounded interleaved external-output block after process. Each
    /// process call must be at most plugin_block_frames() frames.
    pub fn plugin_output(&self) -> &[f32] {
        self.hardware_samples()
    }
    pub const fn plugin_block_frames() -> usize {
        crate::mixer::MAX_BLOCK
    }

    /// MIDI channel selects a rack row. Missing rows are silent. This uses the
    /// engine's existing bounded live-note admission and release behaviour.
    pub fn plugin_midi(&mut self, channel: u8, key: u8, velocity: u8) {
        if key >= 128 || channel >= 16 || velocity >= 128 {
            return;
        }
        let Some(row) = self.plan.channels.get(usize::from(channel)) else {
            return;
        };
        let id = row.id;
        if velocity == 0 {
            self.handle(Message::NoteOff { channel: id, key });
        } else {
            self.handle(Message::NoteOn {
                channel: id,
                key,
                velocity: f32::from(velocity) / 127.0,
            });
        }
    }

    /// Hard reset: no tail, release fade or compensation history survives.
    /// Samples remain retained by their slots/plan; nothing is freed here.
    pub fn reset_plugin(&mut self) {
        self.handle(Message::Stop {
            sequence: self.transport_sequence.wrapping_add(1),
        });
        self.handle(Message::Seek(0.0));
        self.voices.reset_plugin();
        self.clips.reset_plugin();
        for channel in &mut self.state.channel_voice {
            channel.reset();
        }
        for unit in self.state.instrument_units() {
            unit.reset_plugin();
        }
        for unit in self.state.chains.iter_mut().flatten().flatten() {
            unit.reset_plugin();
        }
        for track in &mut self.state.track_processing {
            track.reset();
        }
        for delay in self
            .state
            .direct
            .iter_mut()
            .chain(self.state.external.iter_mut())
            .chain(self.state.edge_delays.iter_mut())
            .chain(self.state.key_delays.iter_mut().flatten())
            .chain([&mut self.state.printed, &mut self.state.master_output])
        {
            if let Some(line) = &mut delay.line {
                line.reset_plugin();
            }
        }
        self.metronome = crate::metronome::Metronome::new();
        self.count_in = None;
        self.mixer
            .clear(self.plan.tracks.len(), crate::mixer::MAX_BLOCK);
    }

    pub fn plugin_position(&self) -> f64 {
        self.sequencer.tick(&self.plan, self.frame)
    }
}
