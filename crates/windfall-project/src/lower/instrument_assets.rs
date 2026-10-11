use super::*;
impl Transaction<'_> {
    pub(super) fn set_instrument_asset(
        &mut self,
        channel: ChannelId,
        expected: ChannelSource,
        asset: Option<windfall_dsp::InstrumentAsset>,
    ) -> Result<Label, CommandError> {
        let index = self.channel_index(channel)?;
        let old = &self.project.channels[index];
        if old.source != expected {
            return Err(CommandError::invalid(
                "instrument source changed while data was prepared",
            ));
        }
        if self
            .project
            .plugin(crate::PluginTarget::Instrument { channel })
            .is_some()
        {
            return Err(CommandError::invalid(
                "a hosted instrument cannot use built-in data",
            ));
        }
        let ChannelSource::Instrument { params, .. } = &old.source else {
            return Err(CommandError::invalid("the channel is not an instrument"));
        };
        if let Some(asset) = &asset {
            asset
                .validate(params.kind())
                .map_err(CommandError::invalid)?;
        }
        self.set_source(
            index,
            ChannelSource::Instrument {
                params: *params,
                asset,
            },
        );
        Ok("Change instrument data")
    }
}
