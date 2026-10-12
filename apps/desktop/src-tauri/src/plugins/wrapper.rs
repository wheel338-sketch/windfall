//! Saved policy to owner/DSP/editor boundaries. Window handles are never saved.
use windfall_plugin_host::{
    EditorOptions,
    wrapper::{ProcessingMode, WrapperOptions},
};
use windfall_project::{PluginBinding, PluginProcessingMode};

pub(super) fn use_bridge(binding: &PluginBinding, default: bool) -> Result<bool, String> {
    binding.wrapper.validate().map_err(str::to_owned)?;
    match binding.wrapper.processing {
        PluginProcessingMode::Automatic => Ok(default),
        PluginProcessingMode::InProcess => Ok(false),
        PluginProcessingMode::Bridged if cfg!(windows) => Ok(true),
        PluginProcessingMode::Bridged => {
            Err("Separate-process hosting is currently Windows-only".into())
        }
        PluginProcessingMode::Threaded => Err(
            "Threaded native processing requires an owner provider that is not implemented yet"
                .into(),
        ),
    }
}
pub(super) fn native(binding: &PluginBinding) -> Result<WrapperOptions, String> {
    binding.wrapper.validate().map_err(str::to_owned)?;
    Ok(WrapperOptions {
        smart_disable: binding.wrapper.smart_disable,
        fixed_block: binding.wrapper.fixed_buffer.map(|frames| frames as usize),
        // The provider above already selected the owning process/thread.
        processing: ProcessingMode::InProcess,
        // Editor options are applied on open, with a current native parent.
        editor: EditorOptions::default(),
    })
}
pub(super) fn max_block(binding: &PluginBinding, maximum: usize) -> usize {
    maximum.max(binding.wrapper.fixed_buffer.unwrap_or(0) as usize)
}
pub(super) fn editor(binding: &PluginBinding, parent: Option<usize>) -> EditorOptions {
    EditorOptions {
        title: binding.name.clone(),
        scale: f64::from(binding.wrapper.editor_scale),
        detached: binding.wrapper.detached,
        owner_window: parent,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_policy_selects_real_owners_and_preserves_native_buffer_and_editor_settings() {
        let mut binding = PluginBinding {
            wrapper: Default::default(),
            target: windfall_project::PluginTarget::Effect {
                effect: windfall_project::EffectId(1),
            },
            format: "clap".into(),
            path: "fixture.clap".into(),
            id: "fixture".into(),
            name: "Fixture".into(),
            state: vec![],
            parameters: vec![],
            sidechain_input: None,
            auxiliary_inputs: vec![],
        };
        assert_eq!(use_bridge(&binding, true), Ok(true));
        assert_eq!(use_bridge(&binding, false), Ok(false));
        binding.wrapper.processing = PluginProcessingMode::InProcess;
        assert_eq!(use_bridge(&binding, true), Ok(false));
        binding.wrapper.processing = PluginProcessingMode::Threaded;
        assert!(use_bridge(&binding, true).is_err());
        binding.wrapper.smart_disable = true;
        binding.wrapper.fixed_buffer = Some(512);
        binding.wrapper.editor_scale = 1.5;
        binding.wrapper.detached = false;
        let native = native(&binding).unwrap();
        assert!(native.smart_disable);
        assert_eq!(native.fixed_block, Some(512));
        assert_eq!(max_block(&binding, 256), 512);
        assert!(editor(&binding, None).validate().is_err());
        let editor = editor(&binding, Some(123));
        assert_eq!(editor.scale, 1.5);
        assert_eq!(editor.owner_window, Some(123));
        assert!(editor.validate().is_ok());
    }
}
