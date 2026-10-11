use super::*;

impl Transaction<'_> {
    pub(super) fn set_sample_playback(&mut self, channel: ChannelId, asset: Option<crate::SamplePlayback>) -> Result<Label, CommandError> {
        let index = self.channel_index(channel)?;
        let old = self.project.channels[index].clone();
        if let Some(asset) = &asset { asset.check(self.project, &old).map_err(CommandError::invalid)?; }
        let mut new = old.clone(); new.sample_playback = asset;
        self.push(Edit::SetChannel(Box::new(Change { old, new })));
        Ok("Edit sample playback")
    }

    pub(super) fn generate_slice_pattern(&mut self, channel: ChannelId, pattern: PatternId) -> Result<Label, CommandError> {
        let index = self.channel_index(channel)?;
        let base = match self.instrument(index)? {
            InstrumentParams::SliceMap(p) => p.base_key,
            InstrumentParams::SliceDeck(p) => p.base_key,
            _ => return Err(CommandError::invalid("Choose a slice instrument.")),
        };
        let asset = self.project.channels[index].sample_playback.as_ref()
            .ok_or_else(|| CommandError::invalid("Load a sample and slice map first."))?;
        if usize::from(base) + asset.slices.len() > 128 {
            return Err(CommandError::invalid("The slice map extends past MIDI key 127. Lower its base key."));
        }
        let mut cursor = 0;
        let notes: Vec<_> = asset.slices.iter().enumerate().map(|(i, slice)| {
            let length = ((slice.end - slice.start) as f64 * asset.source_beats * f64::from(PPQ)).round().max(1.0) as u32;
            let note = NoteInit { start: cursor, length, key: base + i as u8,
                velocity: Some(1.0), pan: Some(0.0), expression: None };
            cursor += length; note
        }).collect();
        if cursor > MAX_PATTERN_TICKS { return Err(CommandError::invalid("Generated slice pattern is too long.")); }
        // Existing notes are deliberately retained; applying twice layers notes.
        self.add_notes(pattern, channel, &notes)?;
        let pattern_index = self.pattern_index(pattern)?;
        let old = PatternInfo::of(&self.project.patterns[pattern_index]);
        let mut new = old.clone(); new.length_steps = new.length_steps.max(cursor.div_ceil(TICKS_PER_STEP));
        self.push(Edit::PatternInfo { id: pattern, change: Change { old, new } });
        Ok("Generate slice pattern")
    }
}
