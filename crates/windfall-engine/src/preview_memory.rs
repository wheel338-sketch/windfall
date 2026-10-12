//! Control-side accounting through queued, playing and retired audition owners.
//! Weak identities never keep sample PCM alive or require callback bookkeeping.
use windfall_core::{AudioBuffer, AudioIdentity};

pub(crate) struct PreviewMemory {
    entries: [Option<(AudioIdentity, u64)>; 256],
    overflowed: bool,
}
impl Default for PreviewMemory {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            overflowed: false,
        }
    }
}
impl PreviewMemory {
    pub fn bytes(&mut self) -> u64 {
        let mut total = 0_u64;
        for entry in &mut self.entries {
            if entry
                .as_ref()
                .is_some_and(|(identity, _)| !identity.is_live())
            {
                *entry = None;
            }
            if let Some((_, bytes)) = entry {
                total = total.saturating_add(*bytes);
            }
        }
        if self.overflowed { u64::MAX } else { total }
    }
    pub fn remember(&mut self, audio: &AudioBuffer) {
        self.bytes();
        let identity = audio.identity();
        if self
            .entries
            .iter()
            .flatten()
            .any(|(existing, _)| *existing == identity)
        {
            return;
        }
        if let Some(slot) = self.entries.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some((identity, audio.retained_bytes()));
        } else {
            // Ordinary previews remain compatible; analysis admission fails
            // closed if their identities exceed this fixed observation table.
            self.overflowed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_accounting_deduplicates_capacity_and_waits_for_actual_retirement() {
        let mut values = Vec::with_capacity(1024);
        values.resize(16, 0.0);
        let audio = AudioBuffer::from_interleaved(8000, 1, values);
        let queued = audio.clone();
        let mut memory = PreviewMemory::default();
        memory.remember(&audio);
        memory.remember(&queued);
        assert_eq!(memory.bytes(), audio.retained_bytes());
        assert!(memory.bytes() >= 4096);
        drop(audio);
        assert!(memory.bytes() >= 4096);
        drop(queued);
        assert_eq!(memory.bytes(), 0);
    }
}
