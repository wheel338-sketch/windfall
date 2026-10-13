//! Loading channel and plugin presets. [`crate::preset`] has the rules.
//!
//! Both commands make their edits and then check the whole project, the way
//! a project file is checked when it loads. A preset that would leave the
//! project invalid fails the command, and a failed command's edits are taken
//! back, so a refused preset changes nothing.

use super::*;
use crate::preset::{ChannelPreset, PluginPreset, PluginPresetState, PresetAudio, Processor};
use crate::{PluginBinding, PluginTarget};

impl Transaction<'_> {
    pub(super) fn apply_channel_preset(
        &mut self,
        id: ChannelId,
        expected: Channel,
        mut preset: ChannelPreset,
        sample: Option<SampleId>,
        audio: Option<PresetAudio>,
        name_color: bool,
    ) -> Result<Label, CommandError> {
        // The audio is whatever `sample` says here, so the body is checked
        // as if it listed the asset it names.
        preset
            .validate(preset.sample_asset)
            .map_err(CommandError::invalid)?;
        let index = self.channel_index(id)?;
        if self.project.channels[index] != expected {
            return Err(CommandError::invalid(
                "The destination channel changed; load the preset again",
            ));
        }
        let mut local = std::collections::HashMap::new();
        if let Some(tables) = &preset.tables {
            let owner = tables
                .owners
                .first()
                .ok_or_else(|| CommandError::invalid("Preset has no channel owner"))?;
            local.insert(*owner, id.0);
        }
        let tables = preset.tables.clone().unwrap_or_default();
        if let ChannelSource::Instrument { params, .. } = &mut preset.source {
            tables
                .remap_midi(self.project, &local, params)
                .map_err(CommandError::invalid)?;
        }
        if let Some(graph) = &mut preset.rack {
            tables
                .remap_rack(self.project, &local, graph)
                .map_err(CommandError::invalid)?;
        }
        if sample.is_some() && audio.is_some() {
            return Err(CommandError::invalid(
                "a preset's audio is a sample of the project or a file, not both",
            ));
        }
        if (sample.is_some() || audio.is_some())
            && !matches!(preset.source, ChannelSource::Sampler(_))
        {
            return Err(CommandError::invalid(
                "only a sampler preset plays a sample",
            ));
        }
        let sample = match (sample, audio) {
            (Some(sample), _) => {
                self.sample_index(sample)?;
                Some(sample)
            }
            (None, Some(audio)) => {
                // The file joins the pool in this same step. A file the
                // project already has is not added again.
                self.add_sample(audio.name, audio.path)?;
                self.created.last().copied().map(SampleId)
            }
            (None, None) => None,
        };
        let target = PluginTarget::Instrument { channel: id };
        let replacing_rack = preset.rack.is_some()
            || self
                .project
                .settings
                .modular_racks
                .iter()
                .any(|rack| rack.channel == id);
        let old = Processor::of_source(&expected.source, self.project.plugin(target));
        let new = Processor::of_source(&preset.source, preset.plugin.as_ref());
        self.remove_automations_of(|automation| match *automation {
            AutomationTarget::InstrumentParam { channel, param } => {
                channel == id
                    && param < crate::scripting::SCRIPT_PARAMETER_BASE
                    && (replacing_rack || !old.keeps_param(&new, param))
            }
            _ => false,
        });
        let loaded = preset.onto(&expected, sample);
        self.push(Edit::SetChannel(Box::new(Change {
            old: expected,
            new: loaded,
        })));
        self.rebind(target, preset.plugin);
        if replacing_rack {
            self.run(Command::SetChannelRack {
                id,
                graph: preset.rack,
                expected: None,
            })?;
        }
        if name_color {
            self.update_channel(
                id,
                ChannelPatch {
                    name: Some(preset.name),
                    color: Some(preset.color),
                    ..ChannelPatch::default()
                },
            )?;
        }
        if preset.tables.is_some() {
            let old = self.project.settings.clone();
            let new = tables
                .apply(self.project, &local, &[id.0])
                .map_err(CommandError::invalid)?;
            self.push(Edit::Settings(Change { old, new }));
        }
        self.project.check().map_err(CommandError::invalid)?;
        Ok("Load channel preset")
    }

    pub(super) fn apply_plugin_preset(
        &mut self,
        target: PluginTarget,
        expected: PluginPresetState,
        mut preset: PluginPreset,
    ) -> Result<Label, CommandError> {
        preset.validate().map_err(CommandError::invalid)?;
        let current = PluginPreset::capture(self.project, target).map_err(CommandError::invalid)?;
        if !current.state.same_destination(&expected) {
            return Err(CommandError::invalid(
                "The destination changed; load the preset again",
            ));
        }
        let mut local = std::collections::HashMap::new();
        let destination = match target {
            PluginTarget::Rack { channel, .. } => channel.0,
            PluginTarget::Instrument { channel } => channel.0,
            PluginTarget::Effect { effect } => effect.0,
        };
        if let Some(tables) = &preset.tables {
            let owner = tables
                .owners
                .first()
                .ok_or_else(|| CommandError::invalid("Preset has no processor owner"))?;
            local.insert(*owner, destination);
        }
        if let PluginPresetState::Instrument { params, .. } = &mut preset.state {
            preset
                .tables
                .clone()
                .unwrap_or_default()
                .remap_midi(self.project, &local, params)
                .map_err(CommandError::invalid)?;
        }
        let old = Processor::of_state(&current.state);
        let new = Processor::of_state(&preset.state);
        match (target, preset.state) {
            (
                target @ PluginTarget::Rack { instrument, .. },
                PluginPresetState::Hosted {
                    instrument: wanted,
                    mut binding,
                },
            ) if instrument == wanted => {
                binding.target = target;
                self.edit_rack_plugin(target, |plugin| {
                    *plugin = binding;
                    Ok(())
                })?;
            }
            (PluginTarget::Rack { .. }, _) => {
                return Err(CommandError::invalid(
                    "A hosted rack node requires a native preset of the same instrument/effect role",
                ));
            }
            (PluginTarget::Instrument { channel }, state) if state.is_instrument() => {
                let index = self.channel_index(channel)?;
                self.remove_automations_of(|automation| match *automation {
                    AutomationTarget::InstrumentParam {
                        channel: moved,
                        param,
                    } => {
                        moved == channel
                            && param < crate::scripting::SCRIPT_PARAMETER_BASE
                            && !old.keeps_param(&new, param)
                    }
                    _ => false,
                });
                match state {
                    PluginPresetState::Instrument { params, asset } => {
                        self.set_source(
                            index,
                            ChannelSource::Instrument {
                                params: checked_instrument(params)?,
                                asset,
                            },
                        );
                        self.rebind(target, None);
                    }
                    PluginPresetState::Hosted { binding, .. } => {
                        self.rebind(target, Some(binding));
                    }
                    PluginPresetState::Effect { .. } => unreachable!("an effect is no instrument"),
                }
            }
            (PluginTarget::Effect { effect }, state) if !state.is_instrument() => {
                let found =
                    self.project
                        .mixer
                        .tracks
                        .iter()
                        .enumerate()
                        .find_map(|(track, strip)| {
                            let slot = strip.effects.iter().position(|slot| slot.id == effect)?;
                            Some((track, slot))
                        });
                let (track, slot) =
                    found.ok_or_else(|| CommandError::not_found("effect", effect))?;
                self.remove_automations_of(|automation| match *automation {
                    AutomationTarget::EffectParam {
                        effect: moved,
                        param,
                        ..
                    } => moved == effect && !old.keeps_param(&new, param),
                    _ => false,
                });
                match state {
                    PluginPresetState::Effect { params, impulse } => {
                        let params = checked_effect(params)?;
                        self.change_track(track, |track| {
                            track.effects[slot].params = params;
                            track.effects[slot].impulse = impulse;
                        });
                        self.rebind(target, None);
                    }
                    PluginPresetState::Hosted { binding, .. } => {
                        self.rebind(target, Some(binding));
                    }
                    PluginPresetState::Instrument { .. } => {
                        unreachable!("an instrument is no effect")
                    }
                }
            }
            (PluginTarget::Instrument { .. }, _) => {
                return Err(CommandError::invalid(
                    "an effect preset cannot be loaded onto an instrument",
                ));
            }
            (PluginTarget::Effect { .. }, _) => {
                return Err(CommandError::invalid(
                    "an instrument preset cannot be loaded onto an effect",
                ));
            }
        }
        if let Some(tables) = preset.tables {
            let old = self.project.settings.clone();
            let new = tables
                .apply(self.project, &local, &[destination])
                .map_err(CommandError::invalid)?;
            self.push(Edit::Settings(Change { old, new }));
        }
        self.project.check().map_err(CommandError::invalid)?;
        Ok("Load plugin preset")
    }

    /// Makes `binding` the hosted plugin at `target`, pointed at it, or
    /// leaves `target` with none.
    fn rebind(&mut self, target: PluginTarget, binding: Option<PluginBinding>) {
        match binding {
            Some(mut binding) => {
                binding.target = target;
                self.bind_plugin(binding);
            }
            None => {
                let old = self.project.plugins.clone();
                let mut new = old.clone();
                new.retain(|plugin| plugin.target != target);
                self.push(Edit::Plugins(Change { old, new }));
            }
        }
    }
}
