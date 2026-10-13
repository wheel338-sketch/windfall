//! Owner-side capability snapshots; saved addresses are merged by the document.
use windfall_project::{PluginAuxInput, PluginParameter};
#[derive(Clone)]
pub(crate) struct Readmission {
    pub parameters: Vec<PluginParameter>,
    pub auxiliary_inputs: Vec<PluginAuxInput>,
    pub outputs: Vec<PluginAuxInput>,
}
pub(super) fn ports(ports: &[windfall_plugin_host::AudioPort]) -> Vec<PluginAuxInput> {
    ports
        .iter()
        .enumerate()
        .filter(|(_, port)| !port.main && port.channels > 0)
        .map(|(index, port)| PluginAuxInput {
            index: index as u32,
            name: port.name.clone(),
            channels: port.channels,
        })
        .collect()
}
pub(super) fn native(instance: &mut windfall_plugin_host::PluginInstance) -> Readmission {
    let parameters = instance
        .params()
        .to_vec()
        .into_iter()
        .filter(|param| !param.hidden)
        .map(|param| PluginParameter {
            id: param.id,
            name: param.name.clone(),
            min: param.min as f32,
            max: param.max as f32,
            value: instance
                .param_value(param.id)
                .unwrap_or(param.default)
                .clamp(param.min, param.max) as f32,
            stepped: param.stepped,
            read_only: param.read_only,
            automatable: param.automatable,
        })
        .collect();
    Readmission {
        parameters,
        auxiliary_inputs: ports(&instance.layout().audio_inputs),
        outputs: ports(&instance.layout().audio_outputs),
    }
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(super) fn described(
    parameters: Vec<windfall_plugin_host::bridge::control::DescribedParameter>,
    layout: windfall_plugin_host::PluginLayout,
) -> Readmission {
    Readmission {
        parameters: parameters
            .into_iter()
            .map(|param| PluginParameter {
                id: param.spec.id,
                name: param.name,
                min: param.spec.min as f32,
                max: param.spec.max as f32,
                value: param.spec.value as f32,
                stepped: param.spec.stepped,
                read_only: param.spec.read_only,
                automatable: param.automatable,
            })
            .collect(),
        auxiliary_inputs: ports(&layout.audio_inputs),
        outputs: ports(&layout.audio_outputs),
    }
}
