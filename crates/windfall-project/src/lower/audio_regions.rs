use super::*;

impl Transaction<'_> {
    pub(super) fn set_sample_regions(
        &mut self,
        sample: SampleId,
        expected: Vec<crate::AudioRegion>,
        regions: Vec<crate::AudioRegion>,
    ) -> Result<Label, CommandError> {
        crate::model::check_audio_regions(&regions).map_err(CommandError::invalid)?;
        let index = self.sample_index(sample)?;
        let old = self.project.samples[index].clone();
        if old.regions != expected {
            return Err(CommandError::invalid(
                "Audio regions changed; reopen the editor.",
            ));
        }
        if old.regions != regions {
            let mut new = old.clone();
            new.regions = regions;
            // Remove+insert at the same index is the ordinary sample metadata
            // edit primitive; history and patches retain complete old/new data.
            self.push(Edit::Sample(ListEdit::Remove { index, item: old }));
            self.push(Edit::Sample(ListEdit::Insert { index, item: new }));
        }
        Ok("Edit audio regions")
    }
}
