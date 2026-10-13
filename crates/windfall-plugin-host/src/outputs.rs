//! Prepared per-bus stereo storage. Mono duplicates; channels beyond stereo
//! retain native processing storage but the stereo mixer reads the first pair.
pub struct OutputBank {
    pub buses: Vec<Box<[[f32; 2]]>>,
}
impl OutputBank {
    pub fn new(frames: usize) -> Self {
        Self {
            buses: (0..crate::MAX_PORTS)
                .map(|_| vec![[0.0; 2]; frames].into_boxed_slice())
                .collect(),
        }
    }
    pub fn clear(&mut self) {
        for bus in &mut self.buses {
            bus.fill([0.0; 2]);
        }
    }
    pub fn copy(&self, bus: u32, output: &mut [[f32; 2]]) -> bool {
        if let Some(source) = self
            .buses
            .get(bus as usize)
            .filter(|source| source.len() >= output.len())
        {
            output.copy_from_slice(&source[..output.len()]);
            true
        } else {
            output.fill([0.0; 2]);
            false
        }
    }
}
