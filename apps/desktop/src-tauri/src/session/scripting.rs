//! Assistant/proposal endpoints and one cancellable, bounded script worker.
//! Tauri wrappers run the slow calls on blocking tasks, never the audio thread.
use super::Session;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use windfall_project::DispatchResult;
use windfall_project::assistant::{
    AssistantContext, AssistantProvider, AssistantReply, LOCAL_HELP, LocalRuleProvider,
};
use windfall_project::proposal::{CommandProposal, ProposalStamp};
use windfall_project::scripting::{
    PianoScriptPreview, PianoScriptRequest, Program, ScriptParameter, piano_preview,
};

impl Session {
    /// Already installed controller scripts run on the controller pump worker.
    /// Snapshot/interpretation happens outside document/hub locks. An entire
    /// pump's edits form a guarded checked batch; feedback is queued afterwards.
    pub(super) fn controller_scripts(
        &self,
        events: Vec<(windfall_project::ControlSource, f32, u8)>,
        stamp: ProposalStamp,
        bank: u32,
        mut commands: Vec<windfall_project::Command>,
    ) -> Result<(), String> {
        let project = {
            let state = self.state();
            if stamp
                != (ProposalStamp {
                    generation: state.generation,
                    revision: state.document.revision(),
                })
            {
                return Err("controller script event became stale".into());
            }
            state.document.project().clone()
        };
        let start = Instant::now();
        let mut feedback = Vec::new();
        for (source, value, number) in events {
            for binding in project
                .settings
                .scripts
                .controllers
                .iter()
                .filter(|binding| super::controllers::source_matches(&binding.source, &source))
            {
                let result = windfall_project::scripting::controller_event(
                    &project,
                    binding,
                    f64::from(value),
                    number,
                    bank,
                    || start.elapsed() > Duration::from_millis(100),
                )?;
                commands.extend(result.commands);
                feedback.extend(result.feedback);
                if commands.len() > 128 || feedback.len() > 128 {
                    return Err("controller script output batch limit exceeded".into());
                }
            }
        }
        let feedback_runtime = if feedback.is_empty() { None } else {
            let runtime = crate::sync::lock(&self.inner.midi_hardware).clone().ok_or("native MIDI output is unavailable for feedback")?;
            if !runtime.state().output_connected { return Err("controller feedback output is disconnected; no project batch applied".into()); }
            Some(runtime)
        };
        let changed = !commands.is_empty();
        let applied = if commands.is_empty() {
            stamp
        } else {
            let result = self.proposal_apply(CommandProposal {
                stamp,
                label: "Controller script".into(),
                explanation: "Installed controller script batch".into(),
                commands,
            })?;
            ProposalStamp {
                revision: result.patch.revision,
                ..stamp
            }
        };
        if !feedback.is_empty() {
            let queued: Result<(), String> = (|| {
                let _recording = self.recording_idle()?;
                let state = self.state();
                if applied != (ProposalStamp { generation: state.generation, revision: state.document.revision() }) { return Err("controller feedback became stale".into()); }
                // Enqueue while the stamp is current; the native worker sends.
                feedback_runtime.expect("feedback preflight").feedback(feedback)
            })();
            if let Err(error) = queued {
                if !changed { return Err(error); }
                self.emit(crate::events::Event::ProjectWarnings(vec![format!("Controller command batch applied; MIDI feedback failed: {error}")]));
            }
        }
        Ok(())
    }

    pub fn proposal_stamp(&self) -> ProposalStamp {
        let state = self.state();
        ProposalStamp {
            generation: state.generation,
            revision: state.document.revision(),
        }
    }

    pub fn assistant_respond(&self, prompt: &str) -> Result<AssistantReply, String> {
        if prompt.len() > 4096 {
            return Err("assistant request byte limit exceeded".into());
        }
        let (document, stamp) = {
            let state = self.state();
            (
                state.document.clone(),
                ProposalStamp {
                    generation: state.generation,
                    revision: state.document.revision(),
                },
            )
        };
        let reply = LocalRuleProvider.respond(
            prompt,
            AssistantContext {
                project: document.project(),
                stamp,
                help: LOCAL_HELP,
            },
        )?;
        if let Some(proposal) = &reply.proposal {
            proposal.validate(&document, stamp, false)?;
        }
        Ok(reply)
    }

    pub fn proposal_apply(&self, proposal: CommandProposal) -> Result<DispatchResult, String> {
        // Registry/size guards before snapshots or engine preparation.
        proposal.batch()?;
        let (document, current) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            (
                state.document.clone(),
                ProposalStamp {
                    generation: state.generation,
                    revision: state.document.revision(),
                },
            )
        };
        let batch = proposal.validate(&document, current, false)?;
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if proposal.stamp
                != (ProposalStamp {
                    generation: state.generation,
                    revision: state.document.revision(),
                })
            {
                return Err("proposal became stale before preparation".into());
            }
            self.sample_edit_ticket(&state, batch, None, Vec::new())?
        };
        let mut prepared = ticket.prepare()?;
        #[cfg(test)]
        self.pause("proposal:prepared");
        let mut retirement = None;
        let result = {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            // Revision also catches no-op edits/history movement while work was
            // pending, in addition to the preparation ticket's source/edit guards.
            if proposal.stamp
                != (ProposalStamp {
                    generation: state.generation,
                    revision: state.document.revision(),
                })
            {
                return Err("proposal became stale during preparation".into());
            }
            prepared.commit(&mut state, &mut retirement)
        };
        if let Some(retirement) = &mut retirement {
            self.retire_project(retirement);
        }
        result
    }

    pub fn script_parameters(&self, source: &str) -> Result<Vec<ScriptParameter>, String> {
        Program::compile(source)
            .map(|p| p.parameters)
            .map_err(|e| e.to_string())
    }

    /// A new run invalidates old completions. Cancel never waits for the worker.
    pub fn script_begin(&self) -> u64 {
        self.inner.script_ticket.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn script_cancel(&self, request: u64) {
        let _ = self.inner.script_ticket.compare_exchange(
            request,
            request.saturating_add(1),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub fn script_preview(
        &self,
        request: u64,
        input: PianoScriptRequest,
    ) -> Result<PianoScriptPreview, String> {
        let _worker = match self.inner.script_worker.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err("a script worker is already running; cancel and retry".into());
            }
        };
        let start = Instant::now();
        let cancelled = || {
            self.inner.script_ticket.load(Ordering::Acquire) != request
                || start.elapsed() > Duration::from_millis(100)
        };
        if cancelled() {
            return Err("script run cancelled".into());
        }
        let (document, stamp) = {
            let state = self.state();
            (
                state.document.clone(),
                ProposalStamp {
                    generation: state.generation,
                    revision: state.document.revision(),
                },
            )
        };
        let result = piano_preview(&document, stamp, &input, cancelled)?;
        if cancelled() {
            return Err("script run cancelled or timed out".into());
        }
        if self.proposal_stamp() != stamp {
            return Err("project changed during script preview".into());
        }
        Ok(result)
    }
}
