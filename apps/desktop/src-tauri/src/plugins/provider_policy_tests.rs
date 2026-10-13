//! Real entry points with the production bridge flag enabled. No helper or
//! native AU is needed: Windows must diagnose the platform before helper lookup.
use super::*;

fn missing_helper() -> std::path::PathBuf {
    // A regular executable file cannot contain a child executable.
    std::env::current_exe().unwrap().join("au-policy-helper")
}

fn binding(target: PluginTarget) -> PluginBinding {
    let component = windfall_plugin_host::au_model::ComponentId {
        kind: u32::from_be_bytes(*if target.is_instrument() {
            b"aumu"
        } else {
            b"aufx"
        }),
        subtype: u32::from_be_bytes(*b"TEST"),
        manufacturer: u32::from_be_bytes(*b"WFAL"),
    };
    PluginBinding {
        target,
        format: "au".into(),
        path: component.path(),
        id: component.id(),
        name: "Policy fixture".into(),
        state: vec![],
        parameters: vec![],
        sidechain_input: None,
        auxiliary_inputs: vec![],
        capabilities: Default::default(),
        wrapper: Default::default(),
    }
}

#[test]
fn production_audio_unit_discovery_checks_the_provider_before_bridge_helper_lookup() {
    let mut runtime = Runtime::new().unwrap();
    runtime.bridge_enabled = true;
    runtime.bridge_helper = Some(missing_helper());
    let binding = binding(PluginTarget::Effect {
        effect: windfall_project::EffectId(1),
    });
    assert_eq!(
        runtime.discover(binding).unwrap_err(),
        "Audio Unit hosting requires macOS"
    );
}

#[test]
fn production_audio_unit_default_preparation_checks_both_roles_before_bridge_lookup() {
    let mut runtime = Runtime::new().unwrap();
    runtime.bridge_enabled = true;
    runtime.bridge_helper = Some(missing_helper());
    let effect = binding(PluginTarget::Effect {
        effect: windfall_project::EffectId(1),
    });
    let instrument = binding(PluginTarget::Instrument {
        channel: windfall_project::ChannelId(1),
    });
    assert_eq!(
        runtime.effect(&effect, 48_000, 128).err().unwrap(),
        "Audio Unit hosting requires macOS"
    );
    assert_eq!(
        runtime.instrument(&instrument, 48_000, 128).err().unwrap(),
        "Audio Unit hosting requires macOS"
    );
}

#[test]
fn production_clap_and_vst3_discovery_and_preparation_keep_default_bridging() {
    let mut runtime = Runtime::new().unwrap();
    runtime.bridge_enabled = true;
    runtime.bridge_helper = Some(missing_helper());
    for format in ["clap", "vst3"] {
        let mut effect = binding(PluginTarget::Effect {
            effect: windfall_project::EffectId(1),
        });
        effect.format = format.into();
        let mut instrument = binding(PluginTarget::Instrument {
            channel: windfall_project::ChannelId(1),
        });
        instrument.format = format.into();
        let expected = "The installed audio helper executable is missing";
        assert_eq!(runtime.discover(effect.clone()).unwrap_err(), expected);
        assert_eq!(
            runtime.effect(&effect, 48_000, 128).err().unwrap(),
            expected
        );
        assert_eq!(
            runtime.instrument(&instrument, 48_000, 128).err().unwrap(),
            expected
        );
    }
}
