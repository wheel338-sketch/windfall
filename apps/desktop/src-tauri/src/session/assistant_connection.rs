//! Per-user BYOK configuration and explicit request entry point. Never persist in Project.
use super::Session;
use windfall_ipc::{AssistantConnection, AssistantConnectionUpdate};
use windfall_project::assistant::{AssistantContext, AssistantReply, LOCAL_HELP};
use windfall_project::proposal::ProposalStamp;

impl Session {
    pub fn assistant_connection(&self) -> Result<AssistantConnection, String> {
        let file = crate::sync::lock(&self.inner.settings).assistant_file();
        self.inner.assistant.status(&file)
    }
    pub fn assistant_configure(
        &self,
        update: AssistantConnectionUpdate,
    ) -> Result<AssistantConnection, String> {
        let file = crate::sync::lock(&self.inner.settings).assistant_file();
        self.inner.assistant.configure(&file, update)
    }
    pub fn assistant_request_begin(&self) -> u64 {
        self.inner.assistant.begin()
    }
    pub fn assistant_request_cancel(&self, request: u64) {
        self.inner.assistant.cancel(request);
    }
    /// Only the explicit UI Ask BYOK action calls this endpoint. No timer,
    /// project load, configuration change or offline help operation calls it.
    pub fn assistant_remote(&self, request: u64, prompt: &str) -> Result<AssistantReply, String> {
        let file = crate::sync::lock(&self.inner.settings).assistant_file();
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
        let reply = self.inner.assistant.respond(
            &file,
            request,
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
        if self.proposal_stamp() != stamp {
            return Err("project changed during assistant request; ask again".into());
        }
        Ok(reply)
    }
}
