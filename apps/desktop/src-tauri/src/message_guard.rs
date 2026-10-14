//! Keeps text that reaches the user from being built as English in Rust.
//!
//! A message with values in it is sent as a stable code and named parameters
//! (`windfall_ipc::ui_message`), so the interface can say it in its own
//! language. Two checks over this crate's own source hold that line:
//!
//! - every `ui_message("code", json!({ … }))` names a registered code and
//!   gives every parameter the code is shown with;
//! - the sentences still built with `format!` are exactly the ones recorded
//!   in `uncoded_messages.txt`. A new one fails here until it is given a
//!   code, or is listed as internal with the reason nobody is shown it; one
//!   that has been coded fails until its line is removed, so the list only
//!   shrinks.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Every non-test Rust source of this crate, with test modules cut off.
fn sources() -> Vec<(String, String)> {
    fn walk(folder: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "tests" {
                    walk(&path, root, out);
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
            // Everything from the first test module on is test code.
            let end = ["#[cfg(test)]", "#[cfg(all(test"]
                .iter()
                .filter_map(|marker| text.find(marker))
                .min()
                .unwrap_or(text.len());
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, text[..end].to_owned()));
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
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

#[test]
fn every_coded_message_names_a_registered_code_and_gives_its_parameters() {
    let mut checked = 0;
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
    }
    assert!(checked >= 25, "only {checked} coded messages were found");
}

#[test]
fn sentences_still_built_in_english_are_exactly_the_recorded_list() {
    let mut found = BTreeSet::new();
    for (path, text) in sources() {
        let mut from = 0;
        while let Some(at) = text[from..].find("format!(") {
            let open = from + at + "format!(".len();
            from = open;
            if let Some((literal, _)) = literal(&text, skip_space(&text, open))
                && parameterised_prose(literal)
            {
                found.insert(format!("{path} | {}", squeezed(literal)));
            }
        }
    }
    let mut internal = BTreeSet::new();
    let mut backlog = BTreeSet::new();
    let mut section = "";
    for line in include_str!("uncoded_messages.txt").lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[internal]" || line == "[backlog]" {
            section = line;
            continue;
        }
        let entry = squeezed(line);
        let fresh = match section {
            "[internal]" => internal.insert(entry),
            "[backlog]" => backlog.insert(entry),
            _ => panic!("an entry before any section: {line}"),
        };
        assert!(fresh, "listed twice: {line}");
    }
    let recorded: BTreeSet<_> = internal.union(&backlog).cloned().collect();
    let new: Vec<_> = found.difference(&recorded).collect();
    let gone: Vec<_> = recorded.difference(&found).collect();
    assert!(
        new.is_empty(),
        "sentences built in English that are not recorded. Send each as a code \
         (windfall_ipc::ui_message), or list it under [internal] with the reason \
         nobody is shown it:\n{new:#?}"
    );
    assert!(
        gone.is_empty(),
        "recorded sentences that are no longer in the source; remove their lines:\n{gone:#?}"
    );
    println!(
        "MESSAGES: {} sentences still built in English, {} internal",
        backlog.len(),
        internal.len()
    );
}
