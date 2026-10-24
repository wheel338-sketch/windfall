//! Native plugin discovery, reviewed loading and support reports.
use crate::{
    NativeEdit, Snapshot,
    args::{self, Args},
    edits::{self, Draft},
    tools::Call,
};
use serde_json::{Value, json};
use windfall_project::{ChannelId, EffectId, PluginTarget, TrackId};

fn target(args: &Args) -> Result<PluginTarget, String> {
    match (
        args::optional_id(args, "channelId")?,
        args::optional_id(args, "effectId")?,
    ) {
        (Some(channel), None) => Ok(PluginTarget::Instrument {
            channel: ChannelId(channel),
        }),
        (None, Some(effect)) => Ok(PluginTarget::Effect {
            effect: EffectId(effect),
        }),
        _ => {
            Err("give exactly one of channelId or effectId for the installed native plugin".into())
        }
    }
}

pub fn list(call: &Call<'_>, args: &Args) -> Result<Value, String> {
    args::known(args, &["search", "offset", "limit"])?;
    let search = args::optional_text(args, "search", 128)?
        .unwrap_or("")
        .to_lowercase();
    let offset = args::optional_u32(args, "offset", 0..=u32::MAX)?.unwrap_or(0) as usize;
    let limit = args::optional_u32(args, "limit", 1..=200)?.unwrap_or(100) as usize;
    let matching: Vec<_> = call
        .host
        .plugins()?
        .into_iter()
        .filter(|plugin| {
            format!("{} {} {}", plugin.name, plugin.vendor, plugin.id)
                .to_lowercase()
                .contains(&search)
        })
        .collect();
    let items: Vec<_> = matching.iter().skip(offset).take(limit).collect();
    Ok(
        json!({"total": matching.len(), "offset": offset, "items": items,
        "nextOffset": (offset.saturating_add(items.len()) < matching.len()).then_some(offset.saturating_add(items.len())),
        "next": "Use add_plugin with an exact path and id. Failed entries require a successful scan in Windfall's plugin manager."}),
    )
}

pub fn add(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &["path", "id", "trackId", "title", "explanation"])?;
    let path = args::text(args, "path", 4096)?;
    let id = args::text(args, "id", 1024)?;
    let track = args::optional_id(args, "trackId")?.map(TrackId);
    let plugin = call
        .host
        .plugins()?
        .into_iter()
        .find(|plugin| plugin.path == path && plugin.id == id)
        .ok_or(
            "Plugin is not in the scanned catalog; call list_plugins and use its exact path and id",
        )?;
    if !plugin.usable {
        return Err(json!({"stage": "catalog", "plugin": plugin,
            "next": "Run get_plugin_diagnostics and rescan this plugin in Windfall's plugin manager."}).to_string());
    }
    if plugin.instrument != track.is_none() {
        return Err(
            "Native instruments create a channel (omit trackId); effects require trackId".into(),
        );
    }
    if let Some(track) = track {
        crate::summary::track_of(snapshot.document.project(), track.0)?;
    }
    let draft = Draft {
        label: "Add native plugin",
        commands: vec![],
        changes: vec![format!(
            "Add {} {} ({}){}",
            if plugin.instrument {
                "instrument"
            } else {
                "effect"
            },
            plugin.name,
            plugin.format,
            track
                .map(|id| format!(" to mixer track {}", id.0))
                .unwrap_or_default()
        )],
        expected: vec![],
    };
    edits::propose_native(
        call,
        snapshot,
        args,
        draft,
        Some(NativeEdit::Plugin {
            path: plugin.path,
            id: plugin.id,
            name: plugin.name,
            track,
        }),
    )
}

pub fn diagnostics(call: &Call<'_>, args: &Args) -> Result<Value, String> {
    args::known(args, &["channelId", "effectId"])?;
    let target = if args.is_empty() {
        None
    } else {
        Some(target(args)?)
    };
    call.host.plugin_diagnostics(target)
}

pub fn editor(call: &Call<'_>, args: &Args) -> Result<Value, String> {
    args::known(args, &["channelId", "effectId", "open"])?;
    call.host.plugin_editor(
        target(args)?,
        args::optional_bool(args, "open")?.unwrap_or(true),
    )
}
