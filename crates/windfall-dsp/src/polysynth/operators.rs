//! Bounded six-operator phase modulation. Feedback/cyclic edges use the previous
//! substep, independent of graph traversal order. No heap graph is copied to audio.
use crate::{EnvelopeParams, Waveform};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use super::{core, runtime::{self, Context, Tone}};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all="camelCase",default)]
pub struct MatrixOperator {
    pub ratio:f32, pub detune_cents:f32, pub level:f32,
    pub feedback:f32, pub key_scale:f32, pub waveform:Waveform, pub envelope:EnvelopeParams,
}
impl Default for MatrixOperator {
    fn default()->Self { Self { ratio:1.0,detune_cents:0.0,level:0.5,feedback:0.0,key_scale:0.0,waveform:Waveform::Sine,envelope:EnvelopeParams::default() } }
}
pub(super) fn configure(operators:&[MatrixOperator;6],tone:&mut Tone,rate:f32) {
    for (op,env) in operators.iter().zip(&mut tone.operators) { runtime::envelope(env,op.envelope,rate); }
}
pub(super) fn matrix(t:&mut Tone,c:&Context,ops:&[MatrixOperator;6],fm:&[[f32;6];6],ring:&[[f32;6];6],u:usize,pitch:f32,lfo_index:f32)->[f32;6] {
    let previous=t.previous[u];let mut now=[0.0;6];
    for (to,op) in ops.iter().enumerate() {
        let inc=core::increment(pitch+12.0*op.ratio.log2()+op.detune_cents/100.0,c.rate);
        let phase=t.oscillators[u][to].phase();
        let modulation=fm[to].iter().zip(previous).map(|(depth,source)|depth*source).sum::<f32>()+op.feedback*previous[to]+lfo_index*c.lfo;
        // Keep phase modulation finite while preserving all small-signal routes.
        let moved=(phase+modulation/std::f32::consts::TAU).rem_euclid(1.0);
        let raw=crate::blocks::oscillator::tonal_sample(op.waveform,moved,inc,0.5);
        t.oscillators[u][to].tick(Waveform::Sine,inc,0.5);
        let mut sample=raw;
        for (from,&depth) in ring[to].iter().enumerate() {
            // Depth 1 is a true ring product, depth 0 leaves the carrier intact.
            sample*=1.0-depth+depth*previous[from];
        }
        let key=(op.key_scale*(c.key as f32-60.0)/12.0).exp2().min(4.0);
        now[to]=core::guard(sample*op.level*t.levels[to]*key);
    }
    t.previous[u]=now;now
}
