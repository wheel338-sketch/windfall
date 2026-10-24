//! The MCP plugin path shares desktop admission and records support evidence.
use super::*;
use serde_json::json;
use windfall_project::{EffectId, PluginTarget, TrackId};

impl Session {
    fn mcp_plugin_manager(&self) -> Result<Arc<crate::plugins::PluginManager>, String> {
        lock(&self.inner.plugins)
            .clone()
            .ok_or("Native plugin hosting is unavailable".into())
    }

    pub(super) fn mcp_plugins(&self) -> Result<Vec<windfall_mcp::PluginInfo>, String> {
        Ok(self
            .mcp_plugin_manager()?
            .state()
            .entries
            .into_iter()
            .map(|entry| windfall_mcp::PluginInfo {
                path: entry.path,
                id: entry.id,
                format: entry.format,
                name: entry.name,
                vendor: entry.vendor,
                instrument: entry.instrument,
                usable: entry.usable,
                error: entry.error,
            })
            .collect())
    }

    pub(super) fn mcp_plugin_diagnostics(
        &self,
        target: Option<PluginTarget>,
    ) -> Result<Value, String> {
        let (generation, revision, bindings) = {
            let state = self.state();
            let bindings: Vec<_> = state
                .document
                .project()
                .plugin_bindings()
                .into_iter()
                .filter(|binding| target.is_none_or(|target| binding.target == target))
                .map(|binding| {
                    json!({"target": binding.target, "name": binding.name, "format": binding.format,
                    "id": binding.id, "path": binding.path, "wrapper": binding.wrapper,
                    "parameterCount": binding.parameters.len(), "stateBytes": binding.state.len(),
                    "unavailableParameters": binding.capabilities.unavailable_parameters})
                })
                .collect();
            (state.generation, state.document.revision(), bindings)
        };
        let mut report = json!({"reportVersion": 1, "capturedAt": chrono::Utc::now().to_rfc3339(),
            "windfallVersion": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "audio": self.engine_status(), "recording": self.recording_state().active,
            "generation": generation, "revision": revision, "requestedTarget": target, "bindings": bindings,
            "lastFailure": lock(&self.inner.mcp.plugin_failure).clone(),
            "share": "Send this complete JSON report with the plugin version and what you clicked. No credentials, song notes or opaque plugin state are included."});
        match self.mcp_plugin_manager() {
            Ok(manager) => {
                let status = manager.state();
                report["hostingAvailable"] = json!(true);
                report["scan"] = json!({"scanning": status.scanning, "completed": status.completed,
                    "total": status.total, "current": status.current, "error": status.error, "folders": status.folders});
                report["catalogCount"] = json!(status.entries.len());
                report["catalogTruncated"] = json!(target.is_none() && status.entries.len() > 200);
                let entries: Vec<_> = status
                    .entries
                    .into_iter()
                    .filter(|entry| {
                        target.is_none()
                            || report["bindings"].as_array().is_some_and(|bindings| {
                                bindings.iter().any(|binding| {
                                    binding["path"] == entry.path && binding["id"] == entry.id
                                })
                            })
                    })
                    .take(200)
                    .collect();
                report["catalog"] = json!(entries);
                report["runtimeRevision"] = json!(manager.runtime.document_revision());
                report["instances"] = json!(status.instances);
                let targets: Vec<_> = if let Some(target) = target {
                    vec![target]
                } else {
                    let state = self.state();
                    state
                        .document
                        .project()
                        .plugin_bindings()
                        .into_iter()
                        .map(|binding| binding.target)
                        .collect()
                };
                report["editorsTruncated"] = json!(targets.len() > 8);
                report["editors"] = json!(
                    targets
                        .into_iter()
                        .take(8)
                        .map(|target| {
                            match manager.runtime.editor_diagnostics(target) {
                                Ok(status) => json!({"target": target, "status": status}),
                                Err(error) => json!({"target": target, "error": error}),
                            }
                        })
                        .collect::<Vec<_>>()
                );
            }
            Err(error) => {
                report["hostingAvailable"] = json!(false);
                report["hostingError"] = json!(error);
            }
        }
        Ok(report)
    }

    pub(crate) fn remember_plugin_failure(
        &self,
        stage: &str,
        target: Option<PluginTarget>,
        plugin: Option<(&str, &str)>,
        error: &str,
    ) {
        *lock(&self.inner.mcp.plugin_failure) = Some(json!({"stage": stage, "target": target,
            "path": plugin.map(|p| p.0), "id": plugin.map(|p| p.1), "error": error,
            "at": chrono::Utc::now().to_rfc3339()}));
    }

    pub(super) fn mcp_plugin_error_report(
        &self,
        stage: &str,
        target: Option<PluginTarget>,
        plugin: Option<(&str, &str)>,
        error: &str,
    ) -> String {
        self.remember_plugin_failure(stage, target, plugin, error);
        let report = self
            .mcp_plugin_diagnostics(target)
            .unwrap_or_else(|diagnostic_error| json!({"diagnosticError": diagnostic_error}));
        json!({"ok": false, "stage": stage, "error": error, "report": report}).to_string()
    }

    /// The UI button and MCP use the exact same recording and native-owner path.
    pub(crate) fn change_plugin_editor(
        &self,
        target: PluginTarget,
        open: bool,
        parent: Option<usize>,
    ) -> Result<(), String> {
        let result = (|| {
            let _recording = self.recording_idle()?;
            let manager = self.mcp_plugin_manager()?;
            let binding = self
                .document_snapshot()
                .project
                .plugin(target)
                .cloned()
                .ok_or("Plugin no longer exists")?;
            manager
                .runtime
                .editor_binding_with_parent(target, Some(binding), open, parent)
        })();
        if let Err(error) = &result {
            self.remember_plugin_failure(
                if open { "editorOpen" } else { "editorClose" },
                Some(target),
                None,
                error,
            );
        }
        result
    }

    pub(super) fn mcp_add_plugin(
        &self,
        proposal: CommandProposal,
        path: &str,
        id: &str,
        track: Option<TrackId>,
    ) -> Result<windfall_project::DispatchResult, String> {
        let mut stage = "proposalAdmission";
        let result = (|| {
            if !proposal.commands.is_empty()
                || proposal.label.trim().is_empty()
                || proposal.label.len() > 128
                || proposal.explanation.len() > 4096
            {
                return Err("Invalid native plugin proposal".into());
            }
            drop(self.recording_idle()?);
            {
                let state = self.state();
                if proposal.stamp.generation != state.generation
                    || proposal.stamp.revision != state.document.revision()
                {
                    return Err("proposal is stale; generate a new preview".into());
                }
            }
            stage = "nativeDiscovery";
            let manager = self.mcp_plugin_manager()?;
            let entry = manager
                .state()
                .entries
                .into_iter()
                .find(|entry| entry.path == path && entry.id == id && entry.usable)
                .ok_or("Scan this plugin successfully before loading it")?;
            if entry.instrument != track.is_none() {
                return Err(
                    "Plugin role does not match the requested instrument/effect destination".into(),
                );
            }
            let target = track.map_or(
                PluginTarget::Instrument {
                    channel: ChannelId(0),
                },
                |_| PluginTarget::Effect {
                    effect: EffectId(0),
                },
            );
            let plugin = manager.binding(path, id, target)?;
            stage = "audioPreparationAndCommit";
            let command = match track {
                Some(track) => Command::AddPluginEffect { track, plugin },
                None => Command::AddPluginInstrument { plugin },
            };
            self.dispatch_file_command(
                Command::Batch {
                    label: Some(proposal.label.clone()),
                    commands: vec![command],
                },
                proposal.stamp.generation,
                proposal.stamp.revision,
            )
        })();
        result.map_err(|error| self.mcp_plugin_error_report(stage, None, Some((path, id)), &error))
    }
}
