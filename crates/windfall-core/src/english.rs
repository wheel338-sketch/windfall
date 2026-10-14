//! The words of coded messages, without the interface.
//!
//! The interface's catalogs are the one source of a coded message's words.
//! Two kinds of reader need them without the interface: tests that check
//! what a refusal or a report line says, so they keep asserting the words a
//! user sees, and the exported plugin, which runs in another program's window
//! and shows its own panel in the language the user chose in Windfall. Tests
//! read a notice through [`english`], which renders it from the English
//! catalog as the interface does in English (numbers written plainly, as
//! Rust writes them); the plugin's panel reads it through [`in_language`].
//! The catalog modules are compiled in; the module exists only with the
//! `english` feature.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::notice::{Arg, NOTICE_PREFIX, decode};

/// A language the interface is shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Spanish,
}

impl Language {
    /// The language a stored interface setting names (`en`, `es`), if
    /// Windfall has it.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim() {
            "en" => Some(Self::English),
            "es" => Some(Self::Spanish),
            _ => None,
        }
    }

    /// The language of a locale name such as `es-ES`, `es_MX.UTF-8` or
    /// `en`, if Windfall has it.
    pub fn from_locale(locale: &str) -> Option<Self> {
        let primary = locale.trim().split(['-', '_', '.', '@']).next()?;
        Self::from_code(&primary.to_ascii_lowercase())
    }
}

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

/// The interface's catalogs of Windfall's own words and fixed sentences that
/// the backend sends as they are: a term inside a coded message, or a
/// sentence of its own. The interface says them in the language shown when
/// they are catalog keys.
const WORDS: &[(&str, &str)] = &[
    (
        "messages/backend-terms.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/messages/backend-terms.ts"),
    ),
    (
        "messages/backend-statics.ts",
        include_str!("../../../apps/desktop/src/lib/i18n/messages/backend-statics.ts"),
    ),
];

/// The entries of `sources` in `language`. The English object comes first
/// in every module and ends at its `as const`; the Spanish one follows it.
fn read(sources: &[(&str, &str)], language: Language) -> HashMap<String, Entry> {
    let mut catalog = HashMap::new();
    for (name, text) in sources {
        let (english, spanish) = text
            .split_once("as const")
            .unwrap_or_else(|| panic!("{name} has no English catalog object"));
        let part = match language {
            Language::English => english,
            Language::Spanish => spanish,
        };
        for line in part.lines() {
            if let Some((code, entry)) = parse_line(line.trim()) {
                catalog.insert(code, entry);
            }
        }
    }
    catalog
}

/// The catalog of every native code in `language`, read once.
fn catalog_in(language: Language) -> &'static HashMap<String, Entry> {
    static ENGLISH: OnceLock<HashMap<String, Entry>> = OnceLock::new();
    static SPANISH: OnceLock<HashMap<String, Entry>> = OnceLock::new();
    let cell = match language {
        Language::English => &ENGLISH,
        Language::Spanish => &SPANISH,
    };
    cell.get_or_init(|| read(SOURCES, language))
}

/// The English catalog of every native code, read once.
fn catalog() -> &'static HashMap<String, Entry> {
    catalog_in(Language::English)
}

/// Windfall's own words and sentences in Spanish, by their English, read
/// once.
fn spanish_words() -> &'static HashMap<String, Entry> {
    static WORDS_ES: OnceLock<HashMap<String, Entry>> = OnceLock::new();
    WORDS_ES.get_or_init(|| read(WORDS, Language::Spanish))
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

/// `text` with every coded message in it in `language`, as the interface
/// shows it in that language; any other text as it is, unless it is one of
/// Windfall's own sentences, which is said in that language too.
///
/// In Spanish a number is written as the interface writes it there: a
/// decimal comma, at most three decimals and groups of three digits from
/// five digits on, except the values that number a thing (an id, a channel)
/// which are written as they are. A code without a Spanish entry is said in
/// English.
///
/// # Panics
///
/// As [`english`], if a code has no English entry either.
pub fn in_language(text: &str, language: Language) -> String {
    match language {
        Language::English => english(text),
        Language::Spanish => {
            if text.contains(NOTICE_PREFIX) {
                spanish(text)
            } else {
                own_sentence(text)
            }
        }
    }
}

/// The values of a coded message that hold a cause: one of Windfall's own
/// sentences is said in the language shown.
const CAUSES: &[&str] = &["error", "errors", "problem", "reason", "detail"];
/// The values that number a thing rather than count it: written as they are.
const IDENTIFIERS: &[&str] = &[
    "id", "handle", "channel", "clip", "effect", "output", "pattern", "track",
];

/// `text` in Spanish when it is one of Windfall's own words or sentences (in
/// lower case inside another message), with any coded message in it in
/// Spanish otherwise.
fn own_sentence(text: &str) -> String {
    let words = spanish_words();
    if let Some(Entry::Text(said)) = words.get(text) {
        return said.clone();
    }
    let mut chars = text.chars();
    if let Some(first) = chars.next() {
        let capital: String = first.to_uppercase().chain(chars).collect();
        if capital != text
            && let Some(Entry::Text(said)) = words.get(&capital)
        {
            let mut said = said.chars();
            return said
                .next()
                .map(|first| first.to_lowercase().chain(said).collect())
                .unwrap_or_default();
        }
    }
    spanish(text)
}

/// A number as the Spanish interface writes it.
fn spanish_number(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    let plain = format!("{}", rounded.abs());
    let (whole, fraction) = plain.split_once('.').unwrap_or((&plain, ""));
    let mut digits = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if whole.len() >= 5 && index > 0 && (whole.len() - index) % 3 == 0 {
            digits.push('.');
        }
        digits.push(digit);
    }
    let sign = if rounded < 0.0 { "-" } else { "" };
    if fraction.is_empty() {
        format!("{sign}{digits}")
    } else {
        format!("{sign}{digits},{fraction}")
    }
}

/// `text` with every coded message in it in Spanish.
fn spanish(text: &str) -> String {
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
            out.push_str(&own_sentence(&notice.text("text").unwrap_or_default()));
        } else if notice.code == "text.sentence" {
            let words = spanish(&notice.text("text").unwrap_or_default());
            let ended = words.ends_with(['.', '!', '?', '…']);
            out.push_str(&words);
            if !ended {
                out.push('.');
            }
        } else {
            let entry = catalog_in(Language::Spanish)
                .get(&notice.code)
                .or_else(|| catalog().get(&notice.code))
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
                let said = match value {
                    Arg::Text(text)
                        if CAUSES.contains(&name.as_str()) && !text.starts_with(NOTICE_PREFIX) =>
                    {
                        own_sentence(text)
                    }
                    Arg::Text(text) => spanish(text),
                    _ if IDENTIFIERS.contains(&name.as_str()) => number(value),
                    Arg::Float(value) => spanish_number(*value),
                    Arg::Int(value) => spanish_number(*value as f64),
                    Arg::UInt(value) => spanish_number(*value as f64),
                };
                words = words.replace(&format!("{{{name}}}"), &said);
            }
            out.push_str(&words);
        }
        rest = &candidate[end..];
    }
    out.push_str(rest);
    out
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
    fn coded_messages_read_in_spanish_with_their_words_and_numbers() {
        let owner = crate::notice!("project.owner.mixerTrack", id = 3_u32);
        let text = crate::notice!(
            "project.check.volume",
            owner = owner,
            value = 2.5_f32,
            max = 4_u32
        )
        .to_string();
        let said = in_language(&text, Language::Spanish);
        assert!(said.contains("2,5"), "{said}");
        assert!(!said.contains("outside"), "{said}");
        let plural = crate::notice!("midi.import.markersNotImported", count = 12_345_u32);
        let said = in_language(&plural.to_string(), Language::Spanish);
        assert_eq!(said, "No se han importado 12.345 marcadores");
        // One of Windfall's own sentences, alone or as a term.
        assert_eq!(
            in_language("Missing host editor parent", Language::Spanish),
            "Falta la ventana padre del editor del host"
        );
        let term = crate::notice::term("Missing host editor parent").to_string();
        assert_eq!(
            in_language(&term, Language::Spanish),
            "Falta la ventana padre del editor del host"
        );
        // Anything else stays as it is; English is what `english` says.
        assert_eq!(
            in_language("C:/Songs/a.json", Language::Spanish),
            "C:/Songs/a.json"
        );
        assert_eq!(in_language(&text, Language::English), english(&text));
        assert_eq!(spanish_number(-0.1234), "-0,123");
        assert_eq!(spanish_number(1234.0), "1234");
        assert_eq!(spanish_number(1_234_567.5), "1.234.567,5");
    }

    #[test]
    fn a_language_is_read_from_a_setting_or_a_locale() {
        assert_eq!(Language::from_code("es"), Some(Language::Spanish));
        assert_eq!(Language::from_code("en"), Some(Language::English));
        assert_eq!(Language::from_code("fr"), None);
        assert_eq!(
            Language::from_locale("es_MX.UTF-8"),
            Some(Language::Spanish)
        );
        assert_eq!(Language::from_locale("es-ES"), Some(Language::Spanish));
        assert_eq!(Language::from_locale("EN-us"), Some(Language::English));
        assert_eq!(Language::from_locale("C"), None);
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
