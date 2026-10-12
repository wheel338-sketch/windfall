//! The multisampler's own chorus, echo and reverb. All buffers prepare off audio.
use crate::{Chorus, ChorusParams, Delay, DelayParams, Reverb, ReverbParams, Effect};

#[derive(Default)]
pub(super) struct BankEffects {
    chorus: Chorus,
    delay: Delay,
    reverb: Reverb,
}
impl BankEffects {
    pub fn prepare(&mut self, rate: f32, block: usize) {
        self.chorus.prepare(rate, block); self.delay.prepare(rate, block); self.reverb.prepare(rate, block);
    }
    pub fn reset(&mut self) { self.chorus.reset(); self.delay.reset(); self.reverb.reset(); }
    pub fn set_params(&mut self, (chorus, delay, reverb): (ChorusParams, DelayParams, ReverbParams)) {
        self.chorus.set_params(&chorus); self.delay.set_params(&delay); self.reverb.set_params(&reverb);
    }
    pub fn set_tempo(&mut self, bpm: f32) { self.chorus.set_tempo(bpm); self.delay.set_tempo(bpm); self.reverb.set_tempo(bpm); }
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.chorus.process(left, right); self.delay.process(left, right); self.reverb.process(left, right);
    }
    pub fn tail_samples(&self) -> usize { self.chorus.tail_samples().saturating_add(self.delay.tail_samples()).saturating_add(self.reverb.tail_samples()) }
}
