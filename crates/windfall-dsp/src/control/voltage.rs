//! Software DC generation. Calibration describes normalized audio samples,
//! never a promise about the voltage or safety of attached hardware.
use super::Controls;
use crate::param::param_set;
use crate::{Effect, ParamSet};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct VoltageSourceParams {
    pub value1: f32,
    pub value2: f32,
    pub value3: f32,
    pub value4: f32,
    pub value5: f32,
    pub value6: f32,
    pub value7: f32,
    pub value8: f32,
    pub armed: bool,
}
impl Default for VoltageSourceParams {
    fn default() -> Self {
        Self {
            value1: 0.0,
            value2: 0.0,
            value3: 0.0,
            value4: 0.0,
            value5: 0.0,
            value6: 0.0,
            value7: 0.0,
            value8: 0.0,
            armed: false,
        }
    }
}
param_set!(VoltageSourceParams, "DC Control", {
    float [value1] "value1" "Output 1" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [value2] "value2" "Output 2" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [value3] "value3" "Output 3" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [value4] "value4" "Output 4" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [value5] "value5" "Output 5" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [value6] "value6" "Output 6" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [value7] "value7" "Output 7" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [value8] "value8" "Output 8" { Fraction, Linear, 0.0, 1.0, 0.0 }
    toggle [armed] "armed" "Enable DC output" { false }
});
pub struct VoltageSource {
    controls: Controls<8>,
}
impl Default for VoltageSource {
    fn default() -> Self {
        Self {
            controls: Controls::new([0.0; 8]),
        }
    }
}
impl VoltageSource {
    pub fn next_values(&mut self) -> [f32; 8] {
        self.controls.tick()
    }
    /// Calibration is affine in normalized audio samples and bounded to full scale.
    pub fn calibrated(value: f32, zero: f32, scale: f32) -> f32 {
        let value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let sample = zero + scale * value;
        if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}
impl Effect for VoltageSource {
    type Params = VoltageSourceParams;
    fn prepare(&mut self, rate: f32, _block: usize) {
        self.controls.prepare(rate);
    }
    fn reset(&mut self) {
        self.controls.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls.set([
            p.value1, p.value2, p.value3, p.value4, p.value5, p.value6, p.value7, p.value8,
        ]);
    }
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}
