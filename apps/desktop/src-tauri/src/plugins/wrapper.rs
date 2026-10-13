//! Saved policy to owner/DSP/editor boundaries. Window handles are never saved.
use windfall_plugin_host::{
    EditorOptions,
    wrapper::{ProcessingMode, WrapperOptions},
};
use windfall_project::{PluginBinding, PluginProcessingMode};

pub(super) fn use_bridge(binding: &PluginBinding, default: bool) -> Result<bool, String> {
    provider(
        binding,
        default,
        cfg!(target_os = "macos"),
        cfg!(any(windows, target_os = "macos", target_os = "linux")),
    )
    .map(|provider| provider == Provider::Bridge)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    Native,
    CocoaAudioUnit,
    Bridge,
}

/// Pure production policy shared by discovery and both preparation roles.
/// Platform capabilities are explicit so Windows tests exercise the Mac route.
fn provider(
    binding: &PluginBinding,
    default: bool,
    macos: bool,
    bridge_supported: bool,
) -> Result<Provider, String> {
    binding.wrapper.validate().map_err(str::to_owned)?;
    if binding.format == "au" {
        if !macos {
            return Err("Audio Unit hosting requires macOS".into());
        }
        if binding.wrapper.processing == PluginProcessingMode::Bridged {
            return Err("Audio Units use the Cocoa-main owner; separate-process Audio Unit hosting is unavailable".into());
        }
        return Ok(Provider::CocoaAudioUnit);
    }
    match binding.wrapper.processing {
        PluginProcessingMode::Automatic => Ok(if default {
            Provider::Bridge
        } else {
            Provider::Native
        }),
        PluginProcessingMode::InProcess | PluginProcessingMode::Threaded => Ok(Provider::Native),
        PluginProcessingMode::Bridged if bridge_supported => Ok(Provider::Bridge),
        PluginProcessingMode::Bridged => {
            Err("Separate-process hosting requires Windows, macOS or Linux".into())
        }
    }
}
pub(super) fn native(binding: &PluginBinding) -> Result<WrapperOptions, String> {
    binding.wrapper.validate().map_err(str::to_owned)?;
    Ok(WrapperOptions {
        smart_disable: binding.wrapper.smart_disable,
        fixed_block: binding.wrapper.fixed_buffer.map(|frames| frames as usize),
        // The provider above already selected the owning process/thread.
        processing: if binding.wrapper.processing == PluginProcessingMode::Threaded {
            ProcessingMode::Threaded
        } else {
            ProcessingMode::InProcess
        },
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
    fn binding() -> PluginBinding {
        PluginBinding {
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
            capabilities: Default::default(),
        }
    }
    #[test]
    fn audio_unit_discovery_with_production_bridging_selects_cocoa_main() {
        let mut binding = binding();
        binding.format = "au".into();
        binding.path = "au://61756678:54455354:5746414C".into();
        binding.id = "61756678:54455354:5746414C".into();
        assert_eq!(
            provider(&binding, true, true, true),
            Ok(Provider::CocoaAudioUnit)
        );
        assert_eq!(
            provider(&binding, false, true, true),
            Ok(Provider::CocoaAudioUnit)
        );
        assert_eq!(
            provider(&binding, true, false, true),
            Err("Audio Unit hosting requires macOS".into())
        );
    }
    #[test]
    fn audio_unit_default_effect_and_instrument_preparation_bypass_the_helper() {
        let mut binding = binding();
        binding.format = "au".into();
        for target in [
            windfall_project::PluginTarget::Effect {
                effect: windfall_project::EffectId(1),
            },
            windfall_project::PluginTarget::Instrument {
                channel: windfall_project::ChannelId(1),
            },
        ] {
            binding.target = target;
            for processing in [
                PluginProcessingMode::Automatic,
                PluginProcessingMode::InProcess,
                PluginProcessingMode::Threaded,
            ] {
                binding.wrapper.processing = processing;
                assert_eq!(
                    provider(&binding, true, true, true),
                    Ok(Provider::CocoaAudioUnit)
                );
            }
            binding.wrapper.processing = PluginProcessingMode::Bridged;
            assert!(
                provider(&binding, true, true, true)
                    .unwrap_err()
                    .contains("Cocoa-main")
            );
        }
    }
    #[test]
    fn clap_and_vst3_discovery_and_preparation_preserve_process_policy() {
        let mut binding = binding();
        for format in ["clap", "vst3"] {
            binding.format = format.into();
            for macos in [false, true] {
                for default in [false, true] {
                    binding.wrapper.processing = PluginProcessingMode::Automatic;
                    assert_eq!(
                        provider(&binding, default, macos, true),
                        Ok(if default {
                            Provider::Bridge
                        } else {
                            Provider::Native
                        })
                    );
                    for processing in [
                        PluginProcessingMode::InProcess,
                        PluginProcessingMode::Threaded,
                    ] {
                        binding.wrapper.processing = processing;
                        assert_eq!(
                            provider(&binding, default, macos, true),
                            Ok(Provider::Native)
                        );
                    }
                    binding.wrapper.processing = PluginProcessingMode::Bridged;
                    assert_eq!(
                        provider(&binding, default, macos, true),
                        Ok(Provider::Bridge)
                    );
                    assert!(provider(&binding, default, macos, false).is_err());
                }
            }
        }
    }
    #[test]
    fn saved_policy_selects_real_owners_and_preserves_native_buffer_and_editor_settings() {
        let mut binding = binding();
        assert_eq!(use_bridge(&binding, true), Ok(true));
        assert_eq!(use_bridge(&binding, false), Ok(false));
        binding.wrapper.processing = PluginProcessingMode::InProcess;
        assert_eq!(use_bridge(&binding, true), Ok(false));
        binding.wrapper.processing = PluginProcessingMode::Threaded;
        assert_eq!(use_bridge(&binding, true), Ok(false));
        binding.wrapper.smart_disable = true;
        binding.wrapper.fixed_buffer = Some(512);
        binding.wrapper.editor_scale = 1.5;
        binding.wrapper.detached = false;
        let native = native(&binding).unwrap();
        assert_eq!(native.processing, ProcessingMode::Threaded);
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
