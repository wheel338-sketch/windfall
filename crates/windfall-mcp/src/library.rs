//! The read tools that name files: the browser's folders, the presets of a
//! SoundFont in them, and the channel presets. An agent never reads a file
//! itself: Windfall checks that every path it names is a file inside the
//! browser's folders or the factory content ([`Host::library_file`]) and
//! reads it for the agent.
//!
//! [`Host::library_file`]: crate::Host::library_file

use serde_json::{Value, json};
use windfall_dsp::sample_bank::soundfont::SoundFontPreset;

use crate::args::{self, Args};
use crate::summary::MAX_PAGE;
use crate::tools::Call;

const DEFAULT_PAGE: u32 = 100;

/// The sixteen General MIDI program families, eight programs each.
pub const GM_FAMILIES: [&str; 16] = [
    "Piano",
    "Chromatic Percussion",
    "Organ",
    "Guitar",
    "Bass",
    "Strings",
    "Ensemble",
    "Brass",
    "Reed",
    "Pipe",
    "Synth Lead",
    "Synth Pad",
    "Synth Effects",
    "Ethnic",
    "Percussive",
    "Sound Effects",
];

/// The kinds `search_library` filters by.
pub const KINDS: [&str; 5] = ["audio", "soundFont", "project", "preset", "other"];

/// The General MIDI family of a preset: bank 0 is the melodic General MIDI
/// set, bank 128 its drum kits. Other banks have no family.
pub fn gm_family(bank: u16, program: u16) -> Option<&'static str> {
    match bank {
        0 if program < 128 => Some(GM_FAMILIES[usize::from(program / 8)]),
        128 => Some("Drum Kits"),
        _ => None,
    }
}

fn preset_json(preset: &SoundFontPreset) -> Value {
    json!({
        "bank": preset.bank,
        "program": preset.program,
        "name": preset.name,
        "family": gm_family(preset.bank, preset.program),
        "generalMidiName": (preset.bank == 0 && preset.program < 128)
            .then(|| windfall_midi::gm::program_name(preset.program as u8)),
    })
}

fn page(args: &Args) -> Result<(usize, usize), String> {
    let offset = args::optional_u32(args, "offset", 0..=u32::MAX)?.unwrap_or(0) as usize;
    let limit = args::optional_u32(args, "limit", 1..=MAX_PAGE)?.unwrap_or(DEFAULT_PAGE) as usize;
    Ok((offset, limit))
}

/// `list_soundfont_presets`.
pub fn soundfont_presets(call: &Call<'_>, args: &Args) -> Result<Value, String> {
    args::known(args, &["path", "search", "offset", "limit"])?;
    let path = call.host.library_file(args::text(args, "path", 4096)?)?;
    let search = args::optional_text(args, "search", 128)?
        .map(str::to_lowercase)
        .filter(|search| !search.trim().is_empty());
    let (offset, limit) = page(args)?;
    let presets = call.host.soundfont_presets(&path)?;
    let matching: Vec<&SoundFontPreset> = presets
        .iter()
        .filter(|preset| {
            search.as_deref().is_none_or(|search| {
                preset.name.to_lowercase().contains(search)
                    || gm_family(preset.bank, preset.program)
                        .is_some_and(|family| family.to_lowercase().contains(search))
            })
        })
        .collect();
    let items: Vec<Value> = matching
        .iter()
        .skip(offset)
        .take(limit)
        .map(|preset| preset_json(preset))
        .collect();
    let next = offset + items.len();
    Ok(json!({
        "path": path,
        "presetCount": presets.len(),
        "total": matching.len(),
        "offset": offset,
        "items": items,
        "nextOffset": (next < matching.len()).then_some(next),
        "next": "load_soundfont_preset loads one into a SoundFont channel; add_channel with soundFont adds a channel playing one.",
    }))
}

/// `search_library`.
pub fn search(call: &Call<'_>, args: &Args) -> Result<Value, String> {
    args::known(args, &["query", "kind", "offset", "limit"])?;
    let query = args::optional_text(args, "query", 1024)?.unwrap_or("");
    let kind = args::optional_text(args, "kind", 16)?;
    if let Some(kind) = kind
        && !KINDS.contains(&kind)
    {
        return Err(format!("argument kind must be one of {}", KINDS.join(", ")));
    }
    let (offset, limit) = page(args)?;
    let found = call.host.search_library(query, kind, offset, limit)?;
    let next = offset + found.items.len();
    Ok(json!({
        "total": found.total,
        "offset": offset,
        "items": found.items,
        "nextOffset": (next < found.total).then_some(next),
        "indexing": found.indexing,
        "truncated": found.truncated,
    }))
}

/// `list_channel_presets`.
pub fn channel_presets(call: &Call<'_>, args: &Args) -> Result<Value, String> {
    args::known(args, &["search", "offset", "limit"])?;
    let search = args::optional_text(args, "search", 128)?
        .map(str::to_lowercase)
        .filter(|search| !search.trim().is_empty());
    let (offset, limit) = page(args)?;
    let presets = call.host.channel_presets()?;
    let matching: Vec<_> = presets
        .iter()
        .filter(|preset| {
            search.as_deref().is_none_or(|search| {
                preset.name.to_lowercase().contains(search)
                    || preset.instrument.to_lowercase().contains(search)
            })
        })
        .collect();
    let items: Vec<Value> = matching
        .iter()
        .skip(offset)
        .take(limit)
        .map(|preset| json!(preset))
        .collect();
    let next = offset + items.len();
    Ok(json!({
        "total": matching.len(),
        "offset": offset,
        "items": items,
        "nextOffset": (next < matching.len()).then_some(next),
    }))
}
