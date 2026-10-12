//! Native caller identities and control-side invocation leases. Neither is persisted.
//! The lease spans blocking preparation and its final admission, preventing a
//! replacement command from overtaking an already authenticated custom IPC.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewWindow};

use crate::{session::Session, sync::lock};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowCaller {
    generation: u64,
    token: String,
}

#[derive(Default)]
struct Calls {
    windows: HashMap<String, String>,
    active: HashMap<String, usize>,
    exclusive: bool,
    closing: HashSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct WindowCalls(Arc<Mutex<Calls>>);

pub(crate) struct CallLease {
    calls: WindowCalls,
    label: String,
    exclusive: bool,
    counted: bool,
}

impl Drop for CallLease {
    fn drop(&mut self) {
        let mut calls = lock(&self.calls.0);
        if self.counted
            && let Some(count) = calls.active.get_mut(&self.label)
        {
            *count -= 1;
            if *count == 0 {
                calls.active.remove(&self.label);
            }
        }
        if self.exclusive {
            calls.exclusive = false;
        }
    }
}

impl WindowCalls {
    pub(crate) fn caller(&self, label: &str, generation: u64) -> Result<WindowCaller, String> {
        let mut calls = lock(&self.0);
        if calls.closing.contains(label) {
            return Err("This window is closing.".into());
        }
        let token = match calls.windows.get(label) {
            Some(token) => token.clone(),
            None => {
                let mut bytes = [0u8; 32];
                getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
                let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                calls.windows.insert(label.to_owned(), token.clone());
                token
            }
        };
        Ok(WindowCaller { generation, token })
    }

    pub(crate) fn closed(&self, label: &str) {
        let mut calls = lock(&self.0);
        calls.windows.remove(label);
        calls.closing.remove(label);
    }

    pub(crate) fn busy(&self, label: &str) -> bool {
        lock(&self.0).active.contains_key(label)
    }
    pub(crate) fn closing(&self, label: &str) -> bool {
        lock(&self.0).closing.contains(label)
    }

    pub(crate) fn dock_target(&self, label: &str) -> Result<Option<String>, String> {
        let mut calls = lock(&self.0);
        if calls.active.contains_key(label) || calls.closing.contains(label) {
            return Err(
                "This window has an operation finishing. Retry close after it completes.".into(),
            );
        }
        calls.closing.insert(label.to_owned());
        Ok(calls.windows.remove(label))
    }

    pub(crate) fn failed_dock(&self, label: &str, token: Option<String>) {
        let mut calls = lock(&self.0);
        calls.closing.remove(label);
        if let Some(token) = token {
            calls.windows.entry(label.to_owned()).or_insert(token);
        }
    }

    fn enter(
        &self,
        label: &str,
        caller: &WindowCaller,
        current_generation: impl FnOnce() -> u64,
        command: &str,
    ) -> Result<CallLease, String> {
        let mut calls = lock(&self.0);
        if caller.generation != current_generation() {
            return Err(crate::session::PROJECT_REPLACED.into());
        }
        if calls.windows.get(label) != Some(&caller.token) {
            return Err(
                "This window was closed or reopened. Reload its project before editing.".into(),
            );
        }
        if command == "window_close" && calls.active.contains_key(label) {
            return Err(
                "This window has an operation finishing. Retry close after it completes.".into(),
            );
        }
        let exclusive = matches!(
            command,
            "project_new" | "project_open" | "flp_open" | "template_new" | "update_install"
        );
        if calls.exclusive || (exclusive && !calls.active.is_empty()) {
            return Err("Another window operation is finishing. Retry when it completes.".into());
        }
        calls.exclusive = exclusive;
        // Docking asks the OS to close this same window. Its own lease must
        // not count as outstanding document work in CloseRequested.
        let counted = !matches!(command, "dock_panel" | "window_close");
        if command == "window_close" {
            calls.windows.remove(label);
            calls.closing.insert(label.to_owned());
        }
        if counted {
            *calls.active.entry(label.to_owned()).or_default() += 1;
        }
        Ok(CallLease {
            calls: self.clone(),
            label: label.to_owned(),
            exclusive,
            counted,
        })
    }
}

impl WindowCaller {
    pub(crate) fn enter(&self, window: &WebviewWindow, command: &str) -> Result<CallLease, String> {
        let session = window.state::<Session>();
        window.state::<WindowCalls>().enter(
            window.label(),
            self,
            || session.window_document_generation(),
            command,
        )
    }

    pub(crate) fn failed_close(&self, window: &WebviewWindow) {
        let registry = window.state::<WindowCalls>();
        let mut calls = lock(&registry.0);
        calls.closing.remove(window.label());
        // The opening remains alive even if the document changed. Never
        // overwrite a newly bootstrapped opening's token.
        calls
            .windows
            .entry(window.label().to_owned())
            .or_insert_with(|| self.token.clone());
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowBootstrap {
    pub generation: u64,
    pub caller: WindowCaller,
    pub snapshot: windfall_project::DocumentSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_table() -> Vec<(String, String)> {
        fn scan(folder: &std::path::Path, rows: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    scan(&path, rows);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                for part in source.split(concat!("#[tauri", "::command")).skip(1) {
                    let function = part.split_once("fn ").expect("command function").1;
                    let name = function.split_once('(').unwrap().0.trim().to_owned();
                    rows.push((name, function.to_owned()));
                }
            }
        }
        let mut rows = Vec::new();
        scan(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut rows,
        );
        rows
    }

    #[test]
    fn every_custom_ipc_has_a_caller_lease_and_refuses_both_stale_identities() {
        let table = command_table();
        let handler = include_str!("commands.rs")
            .split_once("tauri::generate_handler![")
            .unwrap()
            .1
            .split_once(']')
            .unwrap()
            .0;
        for registered in handler
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let name = registered.rsplit("::").next().unwrap();
            assert!(
                table.iter().any(|(command, _)| command == name),
                "{registered}: command definition was not audited"
            );
        }
        let calls = WindowCalls::default();
        let departed = calls.caller("panel-mixer", 19).unwrap();
        calls.closed("panel-mixer");
        let current = calls.caller("panel-mixer", 19).unwrap();
        let mut document =
            windfall_project::Document::new(windfall_project::Project::new("Caller fixture"));
        let before = document.snapshot(None);
        let mut guarded = 0;
        for (command, source) in &table {
            // These two endpoints only read a snapshot. Bootstrap issues the
            // caller token; neither is allowed to invoke a document mutation.
            if command == "document_snapshot" || command == "window_document_snapshot" {
                let body = source
                    .split_once('{')
                    .unwrap()
                    .1
                    .split("\n}")
                    .next()
                    .unwrap();
                assert!(
                    body.contains("session.document_snapshot()")
                        || body.contains("session.window_document_snapshot()")
                );
                assert!(!body.contains("dispatch") && !body.contains("blocking("));
                continue;
            }
            let (signature, body) = source.split_once('{').unwrap();
            assert!(
                signature.contains("caller: crate::window_calls::WindowCaller"),
                "{command}: missing caller argument"
            );
            let first = body.trim_start().lines().next().unwrap();
            assert_eq!(
                first.trim(),
                format!("let _window_call = caller.enter(&window, \"{command}\")?;"),
                "{command}: mutation before guard"
            );
            // Table-driven refusal for every path, with no opportunity to run
            // its document operation after either refusal.
            if calls.enter("panel-mixer", &current, || 20, command).is_ok() {
                document
                    .dispatch(windfall_project::Command::AddPattern { name: None }, None)
                    .unwrap();
            }
            if calls
                .enter("panel-mixer", &departed, || 19, command)
                .is_ok()
            {
                document
                    .dispatch(windfall_project::Command::AddPattern { name: None }, None)
                    .unwrap();
            }
            assert_eq!(
                document.snapshot(None),
                before,
                "{command}: stale caller mutated the document/history"
            );
            guarded += 1;
        }
        assert!(guarded >= 205, "command inventory was unexpectedly reduced");
        // The scanner also sees any newly added command file. Guarding a
        // sample of today's methods cannot satisfy this invariant.
    }

    #[test]
    fn replacement_waits_for_all_custom_calls_and_failed_calls_release_ownership() {
        let calls = WindowCalls::default();
        let main = calls.caller("main", 7).unwrap();
        let detached = calls.caller("panel-mixer", 7).unwrap();
        let pending = calls
            .enter("panel-mixer", &detached, || 7, "plugins_add")
            .unwrap();
        assert!(calls.enter("main", &main, || 7, "project_new").is_err());
        assert!(calls.busy("panel-mixer"));
        drop(pending);
        let replacement = calls.enter("main", &main, || 7, "project_new").unwrap();
        assert!(
            calls
                .enter("panel-mixer", &detached, || 7, "automate")
                .is_err()
        );
        drop(replacement);
        assert!(
            calls
                .enter("panel-mixer", &detached, || 8, "automate")
                .is_err()
        );
        assert!(calls.enter("main", &main, || 7, "dispatch").is_ok());
    }

    #[test]
    fn reopened_label_cannot_reuse_a_departed_window_token() {
        let calls = WindowCalls::default();
        let old = calls.caller("panel-mixer", 3).unwrap();
        calls.closed("panel-mixer");
        let new = calls.caller("panel-mixer", 3).unwrap();
        assert_ne!(old.token, new.token);
        assert!(calls.enter("panel-mixer", &old, || 3, "dispatch").is_err());
        assert!(calls.enter("panel-mixer", &new, || 3, "dispatch").is_ok());
    }

    #[test]
    fn accepted_close_revokes_queued_edits_and_waits_for_running_preparation() {
        let calls = WindowCalls::default();
        let caller = calls.caller("panel-mixer", 3).unwrap();
        let pending = calls
            .enter("panel-mixer", &caller, || 3, "plugins_insert")
            .unwrap();
        assert!(
            calls
                .enter("panel-mixer", &caller, || 3, "window_close")
                .is_err()
        );
        drop(pending);
        let close = calls
            .enter("panel-mixer", &caller, || 3, "window_close")
            .unwrap();
        assert!(
            calls.caller("panel-mixer", 3).is_err(),
            "bootstrap must not revive a closing window"
        );
        assert!(!calls.busy("panel-mixer"));
        assert!(
            calls
                .enter("panel-mixer", &caller, || 3, "dispatch")
                .is_err()
        );
        drop(close);
        calls.closed("panel-mixer");
        let reopened = calls.caller("panel-mixer", 3).unwrap();
        assert_ne!(reopened.token, caller.token);
        assert!(
            calls
                .enter("panel-mixer", &caller, || 3, "dock_panel")
                .is_err()
        );
    }
    #[test]
    fn docking_retires_the_target_opening_and_recovers_from_native_close_failure() {
        let calls = WindowCalls::default();
        let caller = calls.caller("panel-mixer", 3).unwrap();
        let pending = calls
            .enter("panel-mixer", &caller, || 3, "plugins_insert")
            .unwrap();
        assert!(calls.dock_target("panel-mixer").is_err());
        drop(pending);
        let token = calls.dock_target("panel-mixer").unwrap();
        assert!(calls.closing("panel-mixer"));
        assert!(calls.caller("panel-mixer", 3).is_err());
        assert!(
            calls
                .enter("panel-mixer", &caller, || 3, "dispatch")
                .is_err()
        );
        calls.failed_dock("panel-mixer", token);
        assert!(!calls.closing("panel-mixer"));
        assert!(
            calls
                .enter("panel-mixer", &caller, || 3, "dispatch")
                .is_ok()
        );
    }
}
