//! Keeps text that reaches the user from being built as English in Rust.
//!
//! A message with values in it is sent as a stable code and named parameters
//! (`windfall_ipc::ui_message` here, `windfall_core::notice!` in the core
//! crates), so the interface can say it in its own language. Two checks over
//! the source of this crate and of every crate under `crates/` hold that
//! line:
//!
//! - every `ui_message("code", json!({ … }))` and every
//!   `notice!("code", name = …)` names a registered code and gives every
//!   parameter the code is shown with;
//! - the sentences still built in English (with `format!`, `write!` or a
//!   `#[error("…")]` attribute) are exactly the ones recorded in
//!   `uncoded_messages.txt`, each after a comment giving its reason: under
//!   `[internal]` the reason nobody is shown it (a file name, a protocol
//!   line, a prompt for a language model), under `[data]` the reason it is
//!   not a message but a name the project keeps, which the user can rename.
//!   A new one fails here until it is given a code or is recorded with its
//!   reason; one that is gone fails until its line is removed.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `text` without its test code: an item marked `#[cfg(test)]` or
/// `#[cfg(all(test, …))]` (a module, a function, an impl) is cut out; a
/// `mod name;` declaration so marked is left, since its file is skipped by
/// name or is cut the same way.
fn without_tests(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let next = ["#[cfg(test)]", "#[cfg(all(test"]
            .iter()
            .filter_map(|marker| rest.find(marker))
            .min();
        let Some(at) = next else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..at]);
        let item = &rest[at..];
        let semicolon = item.find(';');
        let brace = item.find('{');
        match (brace, semicolon) {
            (Some(open), Some(end)) if end < open => {
                rest = &item[end + 1..];
            }
            (Some(open), _) => {
                let mut depth = 0usize;
                let mut close = item.len();
                for (index, c) in item[open..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                close = open + index + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                rest = &item[close..];
            }
            _ => return out,
        }
    }
}

/// Every non-test Rust source of this crate and of the crates under
/// `crates/`, with test items cut out, labelled by path: `session/files.rs`
/// for this crate, `windfall-project/src/check.rs` for a core crate.
fn sources() -> Vec<(String, String)> {
    fn walk(folder: &Path, root: &Path, label: &str, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "tests" {
                    walk(&path, root, label, out);
                }
                continue;
            }
            if !name.ends_with(".rs")
                || name.ends_with("_tests.rs")
                || name.ends_with("_qa.rs")
                || name == "tests.rs"
                || name == "message_guard.rs"
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((format!("{label}{relative}"), without_tests(&text)));
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    let own = manifest.join("src");
    walk(&own, &own, "", &mut out);
    let crates = manifest.join("../../../crates");
    let mut members: Vec<PathBuf> = std::fs::read_dir(&crates)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("src").is_dir())
        .collect();
    members.sort();
    for member in members {
        let name = member.file_name().unwrap().to_string_lossy().into_owned();
        walk(&member.join("src"), &member, &format!("{name}/"), &mut out);
    }
    assert!(
        out.iter()
            .any(|(path, _)| path == "windfall-project/src/check.rs"),
        "the core crates were not found beside this one"
    );
    out
}

/// The string literal that starts at `text[at]`, unescaped only as far as
/// comparing needs: its raw contents and the index after its closing quote.
fn literal(text: &str, at: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'"') {
        return None;
    }
    let mut index = at + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some((&text[at + 1..index], index + 1)),
            _ => index += 1,
        }
    }
    None
}

fn skip_space(text: &str, mut at: usize) -> usize {
    let bytes = text.as_bytes();
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

/// The index just after the parenthesis that closes the one before `open`,
/// skipping string literals.
fn call_end(text: &str, open: usize) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 1usize;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index = literal(text, index).map_or(bytes.len(), |(_, after)| after);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    bytes.len()
}

/// The index of the first comma at the top level of the call that starts at
/// `open`, if there is one before the call ends.
fn first_comma(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index = literal(text, index).map_or(bytes.len(), |(_, after)| after);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            b',' if depth == 0 => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

/// A sentence with values in it: at least one placeholder, a space, and two
/// words of three letters or more outside the placeholders.
fn parameterised_prose(literal: &str) -> bool {
    let mut outside = String::new();
    let mut placeholders = 0;
    let mut chars = literal.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                continue;
            }
            placeholders += 1;
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
            }
            outside.push(' ');
        } else {
            outside.push(c);
        }
    }
    let words = outside
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| word.len() >= 3)
        .count();
    placeholders > 0 && words >= 2 && outside.trim().contains(' ')
}

/// One line of whitespace for every run of it, as the recorded list has it.
fn squeezed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `call` gives a value named `name`: `name = …` in a `notice!`.
fn gives(call: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(found) = call[from..].find(name) {
        let start = from + found;
        let end = start + name.len();
        from = end;
        let before = call[..start].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        let after = call[end..].trim_start();
        if after.starts_with('=') && !after.starts_with("==") {
            return true;
        }
    }
    false
}

#[test]
fn every_coded_message_names_a_registered_code_and_gives_its_parameters() {
    let mut checked = 0;
    let mut notices = 0;
    for (path, text) in sources() {
        let mut from = 0;
        while let Some(found) = text[from..].find("ui_message(") {
            let open = from + found + "ui_message(".len();
            from = open;
            let Some((code, after)) = literal(&text, skip_space(&text, open)) else {
                // The code is chosen by a match elsewhere in the call.
                continue;
            };
            let declared = windfall_ipc::ui_message_params(code)
                .unwrap_or_else(|| panic!("{path}: unregistered message code {code}"));
            // The parameters as written, up to the end of this call.
            let end = text[after..]
                .find(";")
                .map_or(text.len(), |semicolon| after + semicolon);
            let call = &text[after..end];
            for name in declared {
                assert!(
                    call.contains(&format!("\"{name}\":")),
                    "{path}: message {code} is shown with {name}, which this call does not give"
                );
            }
            checked += 1;
        }
        let mut from = 0;
        while let Some(found) = text[from..].find("notice!(") {
            let open = from + found + "notice!(".len();
            from = open;
            if text[..open - "notice!(".len()].ends_with("macro_rules! ") {
                continue;
            }
            let Some((code, after)) = literal(&text, skip_space(&text, open)) else {
                panic!("{path}: a notice! without a literal code");
            };
            let declared = windfall_ipc::ui_message_params(code)
                .unwrap_or_else(|| panic!("{path}: unregistered message code {code}"));
            let call = &text[after..call_end(&text, after)];
            for name in declared {
                assert!(
                    gives(call, name),
                    "{path}: message {code} is shown with {name}, which this notice! does not give"
                );
            }
            notices += 1;
        }
    }
    assert!(checked >= 25, "only {checked} coded messages were found");
    println!("CODED: {checked} ui_message calls, {notices} notice! calls");
}

/// Every sentence built in English: `format!("…")`, `write!(out, "…")`,
/// `writeln!(out, "…")` and `#[error("…")]`, when the text is a sentence with
/// values in it.
fn english_sentences() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (path, text) in sources() {
        for (marker, after_first_argument) in [
            ("format!(", false),
            ("#[error(", false),
            ("write!(", true),
            ("writeln!(", true),
        ] {
            let mut from = 0;
            while let Some(at) = text[from..].find(marker) {
                let open = from + at + marker.len();
                from = open;
                let start = if after_first_argument {
                    match first_comma(&text, open) {
                        Some(comma) => comma + 1,
                        None => continue,
                    }
                } else {
                    open
                };
                if let Some((literal, _)) = literal(&text, skip_space(&text, start))
                    && parameterised_prose(literal)
                {
                    found.insert(format!("{path} | {}", squeezed(literal)));
                }
            }
        }
    }
    found
}

#[test]
fn sentences_still_built_in_english_are_exactly_the_recorded_list() {
    let found = english_sentences();
    let mut internal = BTreeSet::new();
    let mut data = BTreeSet::new();
    let mut backlog = BTreeSet::new();
    let mut section = "";
    let mut reason = false;
    for line in include_str!("uncoded_messages.txt").lines() {
        let line = line.trim();
        if line.is_empty() {
            reason = false;
            continue;
        }
        if line.starts_with('#') {
            reason = true;
            continue;
        }
        if line == "[internal]" || line == "[data]" || line == "[backlog]" {
            section = line;
            reason = false;
            continue;
        }
        let entry = squeezed(line);
        let fresh = match section {
            "[internal]" => {
                assert!(
                    reason,
                    "an internal entry without a reason above it: {line}"
                );
                internal.insert(entry)
            }
            "[data]" => {
                assert!(reason, "a data entry without a reason above it: {line}");
                data.insert(entry)
            }
            "[backlog]" => backlog.insert(entry),
            _ => panic!("an entry before any section: {line}"),
        };
        assert!(fresh, "listed twice: {line}");
    }
    let recorded: BTreeSet<_> = internal
        .iter()
        .chain(&data)
        .chain(&backlog)
        .cloned()
        .collect();
    let new: Vec<_> = found.difference(&recorded).collect();
    let gone: Vec<_> = recorded.difference(&found).collect();
    assert!(
        new.is_empty(),
        "sentences built in English that are not recorded. Send each as a code \
         (windfall_ipc::ui_message, windfall_core::notice!), or list it under \
         [internal] with the reason nobody is shown it:\n{new:#?}"
    );
    assert!(
        gone.is_empty(),
        "recorded sentences that are no longer in the source; remove their lines:\n{gone:#?}"
    );
    println!(
        "MESSAGES: {} sentences still built in English, {} internal, {} data",
        backlog.len(),
        internal.len(),
        data.len()
    );
}

/// Every key of the English catalog: the quoted names before a colon in the
/// interface's message modules.
fn catalog_keys() -> BTreeSet<String> {
    fn walk(folder: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if !name.ends_with(".ts") || name.ends_with(".test.ts") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text.lines() {
                let line = line.trim_start();
                let Some((raw, after)) = literal(line, 0) else {
                    continue;
                };
                if line[after..].trim_start().starts_with(':')
                    && let Ok(key) = serde_json::from_str::<String>(&format!("\"{raw}\""))
                {
                    out.insert(key);
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/i18n"),
        &mut out,
    );
    assert!(
        out.contains("Plugin state exceeds 64 MiB"),
        "the interface's catalog was not found beside this crate"
    );
    out
}

/// Whether the source just before a string literal puts it where an error
/// is made: `Err(`, `ok_or(`, an error variant (`LoadError::Invalid(`), a
/// refusal, a warning pushed for the user, a `reason:` or `message:` field.
fn error_position(before: &str) -> bool {
    const ENDINGS: [&str; 23] = [
        "Err(",
        "ok_or(",
        "ok_or_else(||",
        "map_err(|_|",
        "::Invalid(",
        "::Load(",
        "::Create(",
        "::Layout(",
        "::Activate(",
        "::Deactivate(",
        "::Encoding(",
        "::Corrupt(",
        "::UnsupportedFormat(",
        "::InvalidInput(",
        "::Unsupported(",
        "invalid(",
        "warnings.push(",
        "errors.push(",
        "notes.push(",
        "Some(",
        "reason:",
        "message:",
        "expect_err(",
    ];
    let before = before.trim_end();
    if ENDINGS.iter().any(|ending| before.ends_with(ending)) {
        return true;
    }
    // `refuse…(`, `Refusal::Variant(` and `SomethingError::Variant(`.
    let Some(call) = before.strip_suffix('(') else {
        return false;
    };
    let word_start = |text: &str| {
        text.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |at| at + 1)
    };
    let name = &call[word_start(call)..];
    if name.starts_with("refuse") {
        return true;
    }
    let Some(owner) = call[..word_start(call)].strip_suffix("::") else {
        return false;
    };
    let owner = &owner[word_start(owner)..];
    !name.is_empty()
        && (owner == "Refusal"
            || (owner.starts_with(|c: char| c.is_ascii_uppercase()) && owner.ends_with("Error")))
}

/// Whether a fixed literal reads as a sentence: no values in it, a space,
/// and at least three words of three letters or more.
fn static_prose(literal: &str) -> bool {
    if literal.replace("{{", "").contains('{') || !literal.trim().contains(' ') {
        return false;
    }
    literal
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| word.len() >= 3)
        .count()
        >= 3
}

#[test]
fn fixed_sentences_made_into_errors_are_in_the_catalog() {
    let keys = catalog_keys();
    let mut checked = 0usize;
    let mut missing = Vec::new();
    for (path, text) in sources() {
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'"' {
                index += 1;
                continue;
            }
            let Some((raw, after)) = literal(&text, index) else {
                break;
            };
            let mut start = index.saturating_sub(60);
            while !text.is_char_boundary(start) {
                start -= 1;
            }
            if static_prose(raw) && error_position(&text[start..index]) {
                let sentence = raw.replace("\\\"", "\"");
                let capitalised = sentence[..1].to_uppercase() + &sentence[1..];
                checked += 1;
                if !keys.contains(&sentence) && !keys.contains(&capitalised) {
                    missing.push(format!("{path}: {sentence}"));
                }
            }
            index = after;
        }
    }
    assert!(
        checked >= 1_000,
        "only {checked} fixed error sentences were found"
    );
    assert!(
        missing.is_empty(),
        "fixed sentences made into errors that the interface cannot translate. \
         Add each to a catalog module (messages/backend-statics.ts) with its \
         Spanish:\n{missing:#?}"
    );
    println!("STATIC: {checked} fixed error sentences, all in the catalog");
}
