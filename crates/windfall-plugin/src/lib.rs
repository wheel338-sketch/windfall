//! Windfall as an externally clocked plugin, separate from the desktop app.
//! State includes the checked project and decoded user-owned assets. No file
//! IO, device opening, controller locks or project compilation occurs in process.

mod clap;
pub mod package;
mod panel;
mod publication;
mod state;
mod vst3;

pub use clap::clap_entry;
pub use state::{EmbeddedSample, MAX_STATE_BYTES, Program};

use windfall_engine::{Controller, Processor};

pub const OUTPUT_PORTS: usize = 16;
pub const OUTPUT_CHANNELS: usize = OUTPUT_PORTS * 2;
pub const MAX_HOST_FRAMES: u32 = 8192;

/// Prepared on the host's main thread. Publication moves complete engines at
/// callback boundaries and returns retired engines to main-thread collection.
pub struct GuestEngine {
    processor: Processor,
    // Retains the queues and owns any retired values. Never called by process.
    _controller: Controller,
    sample_rate: u32,
    latency: u32,
}

impl GuestEngine {
    pub fn activate(program: &Program, sample_rate: f64) -> Result<Self, String> {
        if !sample_rate.is_finite()
            || !(8_000.0..=384_000.0).contains(&sample_rate)
            || sample_rate.fract() != 0.0
        {
            return Err("Host sample rate must be an integral 8000–384000 Hz".into());
        }
        program.validate()?;
        let samples = program.sample_pool();
        // Host clock wins. Retain the original document in state, but remove
        // internal clock/navigation drivers from the detached playback plan.
        let mut playback = program.project.clone();
        playback
            .automations
            .retain(|lane| lane.target != windfall_project::AutomationTarget::Tempo);
        playback.playlist.timeline.markers.clear();
        let (processor, controller) =
            Processor::prepare_plugin(&playback, &samples, sample_rate as u32, OUTPUT_CHANNELS)
                .map_err(|error| error.to_string())?;
        let latency = controller.latency_frames();
        Ok(Self {
            processor,
            _controller: controller,
            sample_rate: sample_rate as u32,
            latency,
        })
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn position_ticks(&self) -> f64 {
        self.processor.plugin_position()
    }
    pub fn sync_transport(&mut self, beats: f64, tempo: f64, playing: bool) -> bool {
        self.processor.sync_plugin_transport(beats, tempo, playing)
    }
    pub fn sync_meter(&mut self, numerator: u16, denominator: u16) -> bool {
        self.processor.set_plugin_meter(numerator, denominator)
    }
    pub fn meter(&self) -> (u16, u16) {
        self.processor.plugin_meter()
    }
    pub fn midi(&mut self, channel: u8, key: u8, velocity: u8) {
        self.processor.plugin_midi(channel, key, velocity);
    }
    pub fn reset(&mut self) {
        self.processor.reset_plugin();
    }

    /// Exactly one bounded engine quantum. The interleaved external outputs
    /// remain borrowed until the next call. Larger host blocks are split by CLAP.
    pub fn render(&mut self, frames: usize) -> Option<&[f32]> {
        if frames > Processor::plugin_block_frames() {
            return None;
        }
        let mut main = [0.0; 512];
        self.processor.process(&mut main[..frames * 2]);
        Some(self.processor.plugin_output())
    }
}
