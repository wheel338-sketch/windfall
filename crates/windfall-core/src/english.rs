//! The English words of coded messages.
//!
//! The interface's English catalog is the one source of a coded message's
//! English. Two kinds of reader need those words without the interface:
//! tests that check what a refusal or a report line says, so they keep
//! asserting the words a user sees, and the exported plugin, which runs in
//! another program's window, has no interface language and shows English in
//! its own panel. Both read a notice through [`english`], which renders it
//! from that catalog as the interface does in English (numbers written
//! plainly, as Rust writes them). The catalog modules are compiled in; the
//! module exists only with the `english` feature.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::notice::{Arg, NOTICE_PREFIX, decode};

/// One entry: its text, or its singular and plural forms.
enum Entry {
    Text(String),
    Plural(String, String),
}

/// The interface's catalog modules of native codes, as they are in the
/// source tree. `english_sources.py` writes this list; a test checks it
/// names every module there is.
const SOURCES: &[(&str, &str)] = &[
    (
        "native-messages.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native-messages.ts"),
    ),
    (
        "native/common.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native/common.ts"),
    ),
    (
        "native/desktop.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native/desktop.ts"),
    ),
    (
        "native/engine.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native/engine.ts"),
    ),
    (
        "native/flp.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native/flp.ts"),
    ),
    (
        "native/media.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native/media.ts"),
    ),
    (
        "native/plugins.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native/plugins.ts"),
    ),
    (
        "native/project.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/native/project.ts"),
    ),
];

/// The English catalog of every native code, read once.
fn catalog() -> &'static HashMap<String, Entry> {
    static CATALOG: OnceLock<HashMap<String, Entry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut catalog = HashMap::new();
        for (_, text) in SOURCES {
            // The English object comes first in every module and ends at
            // its `as const`.
            let english = text
                .split("as const")
                .next()
                .expect("an English catalog object");
            for line in english.lines() {
                if let Some((code, entry)) = parse_line(line.trim()) {
                    catalog.insert(code, entry);
                }
            }
        }
        catalog
    })
}

/// A JSON string literal at the start of `text`: its value and the rest.
fn string(text: &str) -> Option<(String, &str)> {
    let rest = text.strip_prefix('"')?;
    let mut value = String::new();
    let mut chars = rest.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((value, &rest[index + 1..])),
            '\\' => match chars.next()?.1 {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                'u' => {
                    let hex: String = (0..4)
                        .filter_map(|_| chars.next().map(|(_, c)| c))
                        .collect();
                    value.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => value.push(other),
            },
            c => value.push(c),
        }
    }
    None
}

/// `"code": "text",` or `"code": { one: "…", other: "…" },`.
fn parse_line(line: &str) -> Option<(String, Entry)> {
    let (code, rest) = string(line)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    if let Some(rest) = rest.strip_prefix('{') {
        let rest = rest.trim_start().strip_prefix("one:")?.trim_start();
        let (one, rest) = string(rest)?;
        let rest = rest.trim_start().strip_prefix(',')?.trim_start();
        let rest = rest.strip_prefix("other:")?.trim_start();
        let (other, _) = string(rest)?;
        Some((code, Entry::Plural(one, other)))
    } else {
        let (text, _) = string(rest)?;
        Some((code, Entry::Text(text)))
    }
}

/// A number as Rust's `format!` wrote it before it was coded: a float that
/// came from an `f32` in its shortest `f32` form.
fn number(value: &Arg) -> String {
    match value {
        Arg::Float(value) if value.is_finite() && f64::from(*value as f32) == *value => {
            (*value as f32).to_string()
        }
        Arg::Float(value) => value.to_string(),
        Arg::Int(value) => value.to_string(),
        Arg::UInt(value) => value.to_string(),
        Arg::Text(text) => english(text),
    }
}

/// `text` with every coded message in it in English, as the interface shows
/// it in English; any other text as it is.
///
/// # Panics
///
/// If a code has no English entry, or the catalog cannot be read.
pub fn english(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(NOTICE_PREFIX) {
        out.push_str(&rest[..at]);
        let candidate = &rest[at..];
        let end = object_end(&candidate[NOTICE_PREFIX.len()..])
            .map(|end| NOTICE_PREFIX.len() + end)
            .unwrap_or(candidate.len());
        let Some(notice) = decode(&candidate[..end]) else {
            out.push_str(&candidate[..end]);
            rest = &candidate[end..];
            continue;
        };
        if notice.code == "text.term" {
            out.push_str(&english(&notice.text("text").unwrap_or_default()));
        } else if notice.code == "text.sentence" {
            let words = english(&notice.text("text").unwrap_or_default());
            let ended = words.ends_with(['.', '!', '?', '…']);
            out.push_str(&words);
            if !ended {
                out.push('.');
            }
        } else {
            let entry = catalog()
                .get(&notice.code)
                .unwrap_or_else(|| panic!("no English entry for {}", notice.code));
            let template = match entry {
                Entry::Text(text) => text,
                Entry::Plural(one, other) => {
                    let count = notice.param("count");
                    if matches!(count, Some(Arg::UInt(1) | Arg::Int(1))) {
                        one
                    } else {
                        other
                    }
                }
            };
            let mut words = template.clone();
            for (name, value) in &notice.params {
                words = words.replace(&format!("{{{name}}}"), &number(value));
            }
            out.push_str(&words);
        }
        rest = &candidate[end..];
    }
    out.push_str(rest);
    out
}

/// `text` in English as the interface shows a message on its own: like
/// [`english`], starting with a capital. A message inside another keeps the
/// case its catalog entry has, so most project errors start in lower case.
pub fn shown(text: &str) -> String {
    let words = english(text);
    let mut chars = words.chars();
    match chars.next() {
        Some(first) if text.starts_with(NOTICE_PREFIX) => {
            first.to_uppercase().chain(chars).collect()
        }
        _ => words,
    }
}

/// The length of the JSON object at the start of `text`, if it is one.
fn object_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_module_of_native_codes_is_compiled_in() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/src/lib/i18n/native");
        let mut modules: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| format!("native/{}", entry.unwrap().file_name().to_string_lossy()))
            .collect();
        modules.push("native-messages.ts".to_owned());
        modules.sort();
        let mut compiled: Vec<String> =
            SOURCES.iter().map(|(name, _)| (*name).to_owned()).collect();
        compiled.sort();
        assert_eq!(compiled, modules);
    }

    #[test]
    fn coded_messages_read_in_english_with_nested_ones_and_plain_numbers() {
        let owner = crate::notice!("project.owner.mixerTrack", id = 3_u32);
        let text = crate::notice!(
            "project.check.volume",
            owner = owner,
            value = 2.5_f32,
            max = 4_u32
        )
        .to_string();
        assert_eq!(
            english(&text),
            "mixer track 3 has volume 2.5, outside 0 to 4"
        );
        let plural = crate::notice!("midi.import.markersNotImported", count = 1234_u32);
        assert_eq!(english(&plural.to_string()), "1234 markers not imported");
        let joined = format!("{plural}; plain words; {owner}");
        assert_eq!(
            english(&joined),
            "1234 markers not imported; plain words; mixer track 3"
        );
        assert_eq!(english("no code here"), "no code here");
        let term = crate::notice::term("Cutoff");
        assert_eq!(english(&term.to_string()), "Cutoff");
        assert_eq!(number(&Arg::Float(f64::from(0.1_f32))), "0.1");
    }
}
