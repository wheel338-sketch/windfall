//! Built-in, resource-free preset semantics for the browser memory library.
use crate::{Reply, json, parse, with_document};
use serde::Deserialize;
use windfall_project::{
    Channel, ChannelId, ChannelPreset, ChannelSource, Command, PluginPreset, PluginPresetState,
    PluginTarget, Preset,
};
const NATIVE: &str = "Audio assets and native plugin state require the desktop app";
fn supported(preset: &Preset) -> Result<(), String> {
    if !preset.assets.is_empty() || preset.mixer_track.is_some() {
        return Err(NATIVE.into());
    }
    if let Some(channel) = &preset.channel {
        if channel.sample_asset.is_some()
            || channel.plugin.is_some()
            || matches!(
                &channel.source,
                ChannelSource::Instrument { asset: Some(_), .. }
            )
        {
            return Err(NATIVE.into());
        }
    }
    if let Some(plugin) = &preset.plugin {
        if matches!(
            &plugin.state,
            PluginPresetState::Hosted { .. }
                | PluginPresetState::Instrument { asset: Some(_), .. }
                | PluginPresetState::Effect {
                    impulse: Some(_),
                    ..
                }
        ) {
            return Err(NATIVE.into());
        }
    }
    Ok(())
}
pub fn doc_preset_capture(handle: u32, input: &str) -> Reply {
    #[derive(Deserialize)]
    struct Request {
        channel: Option<ChannelId>,
        target: Option<PluginTarget>,
    }
    let request: Request = parse("preset capture", input)?;
    with_document(handle, |doc| {
        let preset = if let Some(channel) = request.channel {
            let (body, audio) = ChannelPreset::capture(doc.project(), channel)?;
            if audio.is_some() {
                return Err(NATIVE.into());
            }
            Preset::of_channel(&body.name.clone(), body, None)
        } else {
            let target = request.target.ok_or("Select a plugin")?;
            let body = PluginPreset::capture(doc.project(), target)?;
            Preset::of_plugin("Plugin preset", body)
        };
        supported(&preset)?;
        preset.validate()?;
        json(&windfall_ipc::PresetCapture {
            preset,
            audio: None,
        })
    })?
}
pub fn preset_parse(_handle: u32, input: &str) -> Reply {
    let text: String = parse("preset text", input)?;
    let (preset, notes) = Preset::from_json(text.as_bytes())?;
    supported(&preset)?;
    json(&windfall_ipc::PresetReview {
        preset,
        notes,
        missing_audio: Vec::new(),
    })
}
pub fn preset_entry(_handle: u32, input: &str) -> Reply {
    #[derive(Deserialize)]
    struct Request {
        path: String,
        preset: Preset,
    }
    let r: Request = parse("preset entry", input)?;
    r.preset.validate()?;
    supported(&r.preset)?;
    json(&windfall_ipc::PresetEntry {
        path: r.path,
        name: r.preset.name.clone(),
        kind: r.preset.kind,
        plugin: r.preset.plugin_identity(),
    })
}
pub fn doc_preset_command(handle: u32, input: &str) -> Reply {
    #[derive(Deserialize)]
    struct Request {
        preset: Preset,
        channel: Option<ChannelId>,
        expected: Option<Channel>,
        target: Option<PluginTarget>,
        name_color: bool,
    }
    let r: Request = parse("preset apply", input)?;
    r.preset.validate()?;
    supported(&r.preset)?;
    with_document(handle, |doc| {
        let command = if let Some(preset) = r.preset.channel {
            Command::ApplyChannelPreset {
                id: r.channel.ok_or("Select a channel")?,
                expected: r.expected.ok_or("The channel was not captured")?,
                preset,
                sample: None,
                audio: None,
                name_color: r.name_color,
            }
        } else {
            let target = r.target.ok_or("Select a plugin")?;
            let expected = PluginPreset::capture(doc.project(), target)?.state;
            if matches!(&expected, PluginPresetState::Hosted { .. }) {
                return Err(NATIVE.into());
            }
            Command::ApplyPluginPreset {
                target,
                expected,
                preset: r.preset.plugin.ok_or("Choose a channel or plugin preset")?,
            }
        };
        json(&command)
    })?
}
