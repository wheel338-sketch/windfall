//! A prepared, bounded multichannel block. Conversion happens after summing.
use crate::mixer::{Frame, MAX_BLOCK};
use windfall_project::ExternalOutputRoute;
pub(crate) const MAX_OUTPUT_CHANNELS: usize = 256;

pub(crate) struct HardwareOutput {
    channels: usize,
    frames: usize,
    pub scratch: Box<[Frame]>,
    samples: Box<[f32]>,
}
impl HardwareOutput {
    pub fn new(channels: usize) -> Self {
        let channels = channels.clamp(1, MAX_OUTPUT_CHANNELS);
        Self {
            channels,
            frames: 0,
            scratch: vec![[0.0; 2]; MAX_BLOCK].into_boxed_slice(),
            samples: vec![0.0; channels * MAX_BLOCK].into_boxed_slice(),
        }
    }
    pub fn rewind(&mut self) {
        self.frames = 0;
        self.samples.fill(0.0);
    }
    pub fn position(&self) -> usize {
        self.frames
    }
    pub fn advance(&mut self, frames: usize) {
        self.frames = (self.frames + frames).min(MAX_BLOCK);
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    pub fn samples(&self) -> &[f32] {
        &self.samples[..self.frames * self.channels]
    }
    pub fn add(&mut self, route: ExternalOutputRoute, offset: usize, frame: Frame) {
        if offset >= MAX_BLOCK
            || usize::from(route.left) >= self.channels
            || route
                .right
                .is_some_and(|right| usize::from(right) >= self.channels)
        {
            return;
        }
        let base = offset * self.channels;
        if let Some(right) = route.right {
            self.samples[base + usize::from(route.left)] += frame[0];
            self.samples[base + usize::from(right)] += frame[1];
        } else {
            self.samples[base + usize::from(route.left)] += (frame[0] + frame[1]) * 0.5;
        }
    }
}
