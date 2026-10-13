//! Runtime-only audition target. Native callbacks never enter the session.
use super::{Session, WeakSession};
use crate::sync::lock;
use std::sync::Arc;
use windfall_engine::midi_hardware::{Audition, Runtime};
use windfall_ipc::{MidiHardwareSettings, MidiHardwareState};
use windfall_project::ChannelId;

struct Destination(WeakSession);
impl Audition for Destination {
    fn note(&self, epoch: u64, key: u8, velocity: u8) -> bool {
        self.0
            .upgrade()
            .is_some_and(|session| session.hardware_note(epoch, key, velocity))
    }

    fn note_at(&self, epoch: u64, key: u8, velocity: u8, at: std::time::Instant) -> bool {
        self.0
            .upgrade()
            .is_some_and(|session| session.hardware_note_at(epoch, key, velocity, at))
    }

    fn message(&self, input: windfall_engine::midi_hardware::InputMessage) -> bool {
        self.0
            .upgrade()
            .is_some_and(|session| {
                let linked=session.controller_message(input);
                if !linked {session.soundfont_hardware_message(input);}
                linked
            })
    }

    fn idle(&self, now: std::time::Instant, playhead: &windfall_engine::Playhead) {
        if let Some(session) = self.0.upgrade() {
            session.controller_idle(now, playhead);
        }
    }
}

impl Session {
    pub(super) fn soundfont_hardware_message(&self,input:windfall_engine::midi_hardware::InputMessage) {
        use windfall_engine::midi_hardware::MidiMessage;
        use windfall_dsp::sample_bank::SoundFontMidi;
        if input.device!=0 {return;} // Additional ports keep explicit controller-link routing.
        let (midi_channel,message)=match input.message {
            MidiMessage::Controller {channel,number,value}=>(channel,SoundFontMidi::Controller{number,value}),
            MidiMessage::ChannelPressure {channel,value}=>(channel,SoundFontMidi::Pressure(value)),
            MidiMessage::PolyPressure {channel,key,value}=>(channel,SoundFontMidi::PolyPressure{key,value}),
            MidiMessage::PitchBend {channel,value}=>(channel,SoundFontMidi::PitchWheel(value)),
            _=>return,
        };
        if self.store().settings().midi_hardware.input_channel.is_some_and(|filter|filter!=midi_channel+1) {return;}
        let state=self.state();let Some(target)=state.midi_target else {return;};
        if !state.document.project().channel(target).is_some_and(|c|
            matches!(c.source,windfall_project::ChannelSource::Instrument{params,..} if params.kind()==windfall_project::InstrumentKind::SoundFont)) {return;}
        let epoch=self.controller().hardware_epoch();
        if !self.controller().hardware_soundfont(epoch,target,message) {
            self.emit(crate::events::Event::ProjectWarnings(vec!["SoundFont MIDI controller queue is full; movement was refused.".into()]));
        }
    }
    pub fn start_midi_hardware(&self) -> Result<(), String> {
        let _configuring = lock(&self.inner.midi_configuring);
        if lock(&self.inner.midi_hardware).is_some() {
            return Ok(());
        }
        let settings = self.store().settings().midi_hardware.clone();
        let runtime = Runtime::start(
            self.controller().clone(),
            Arc::new(Destination(self.downgrade())),
            settings,
        )?;
        *lock(&self.inner.midi_hardware) = Some(Arc::new(runtime));
        // The worker lists its ports within a turn or two; until then the
        // inputs go by their ids. Configuring again reads the names.
        let settings = self.store().settings().midi_hardware.clone();
        self.controllers_devices(&settings, &[]);
        Ok(())
    }

    pub(super) fn midi_runtime(&self) -> Result<Arc<Runtime>, String> {
        lock(&self.inner.midi_hardware)
            .clone()
            .ok_or("Native MIDI runtime is unavailable.".into())
    }

    pub fn midi_hardware_state(&self) -> MidiHardwareState {
        let runtime = lock(&self.inner.midi_hardware).clone();
        let mut result = runtime.map_or_else(
            || MidiHardwareState {
                settings: self.store().settings().midi_hardware.clone(),
                error: Some("Native MIDI runtime is unavailable.".into()),
                ..MidiHardwareState::default()
            },
            |runtime| runtime.state(),
        );
        result.external_bpm = self.external_bpm();
        let state = self.state();
        result.target = state.midi_target;
        result.generation = state.generation;
        result
    }

    pub fn midi_hardware_refresh(&self) -> Result<MidiHardwareState, String> {
        let mut result = self.midi_runtime()?.refresh()?;
        let state = self.state();
        result.target = state.midi_target;
        result.generation = state.generation;
        Ok(result)
    }

    pub fn midi_hardware_configure(
        &self,
        settings: MidiHardwareSettings,
    ) -> Result<MidiHardwareState, String> {
        settings.validate()?;
        let _configuring = lock(&self.inner.midi_configuring);
        // Recording exclusion is retained while devices change, always before state.
        let _recording = self.recording_idle()?;
        let runtime = self.midi_runtime()?;
        let mut result = runtime.configure(settings.clone())?;
        self.controllers_devices(&settings, &result.inputs);
        result.external_bpm = self.external_bpm();
        self.store().update(|store| store.midi_hardware = settings);
        let state = self.state();
        result.target = state.midi_target;
        result.generation = state.generation;
        Ok(result)
    }

    /// Both project identity and revision are checked; IDs can recur in a new document.
    pub fn midi_hardware_target(
        &self,
        channel: Option<ChannelId>,
        generation: u64,
        revision: u64,
    ) -> Result<MidiHardwareState, String> {
        let _recording = self.recording_idle()?;
        {
            let mut state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("Project changed. Refresh the MIDI destination and try again.".into());
            }
            if channel.is_some_and(|id| state.document.project().channel(id).is_none()) {
                return Err("MIDI destination no longer exists.".into());
            }
            self.controller().panic_hardware();
            state.midi_target = channel;
        }
        Ok(self.midi_hardware_state())
    }

    pub fn midi_hardware_panic(&self) {
        self.controller().panic_hardware();
    }

    pub(super) fn hardware_note(&self, epoch: u64, key: u8, velocity: u8) -> bool {
        let state = self.state();
        let Some(channel) = state.midi_target else {
            return false;
        };
        if epoch != self.controller().hardware_epoch()
            || state.document.project().channel(channel).is_none()
        {
            return false;
        }
        let accepted = self
            .controller()
            .hardware_note(epoch, channel, key, velocity);
        if accepted {
            // Lets the UI log what was played, for a later snapped dump.
            self.emit(crate::events::Event::MidiHardwareNote(
                windfall_ipc::MidiHardwareNote {
                    channel,
                    key,
                    velocity,
                },
            ));
        }
        accepted
    }
}
