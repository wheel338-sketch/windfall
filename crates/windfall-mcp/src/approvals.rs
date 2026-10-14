//! Proposals waiting for the user, and the agents the user lets apply
//! without asking.
//!
//! An edit tool never changes the project itself: it files a proposal
//! here. The user applies or rejects it in Windfall's panel, or, once the
//! user has allowed an agent to apply without asking, the agent's own
//! proposals are applied as they are filed. Either way the apply goes
//! through [`Host::apply`](crate::Host::apply), one undo step each.
//!
//! What the user allows lasts until Windfall quits or the server is turned
//! off. An agent is told apart by the name its client gives; that name is
//! the client's own word, so the allowance is a convenience between the
//! user and the agents they connected, not a security boundary. The
//! bearer token is.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use windfall_project::proposal::CommandProposal;

use crate::Host;

/// Proposals that may wait for the user at once.
pub const MAX_WAITING: usize = 16;
/// Decided proposals kept, so an agent can still ask how one ended.
const MAX_KEPT: usize = 64;
/// Agent names remembered for the panel.
const MAX_AGENTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    /// Waiting for the user.
    Pending,
    /// Being applied right now.
    Applying,
    Applied {
        revision: u64,
    },
    Rejected,
    /// The agent took it back, or the server was turned off.
    Withdrawn,
    Failed {
        error: String,
    },
}

impl Status {
    pub fn decided(&self) -> bool {
        !matches!(self, Status::Pending | Status::Applying)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Applying => "applying",
            Status::Applied { .. } => "applied",
            Status::Rejected => "rejected",
            Status::Withdrawn => "withdrawn",
            Status::Failed { .. } => "failed",
        }
    }
}

/// What a proposal will do, worked out on a copy of the project.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// One line per change, in words.
    pub changes: Vec<String>,
    /// The ids the proposal will create, by kind.
    pub created: Value,
    /// How many of each kind of thing there are before and after, for the
    /// kinds that change, and the tempo and meter when they change.
    pub counts: BTreeMap<String, [Value; 2]>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub agent: String,
    pub proposal: CommandProposal,
    pub preview: Preview,
    pub status: Status,
    /// Applied without asking, because the user allowed the agent to.
    pub automatic: bool,
    pub filed: Instant,
}

/// Who asks for a proposal to be applied.
#[derive(Debug, Clone, Copy)]
pub enum Approver<'a> {
    /// The user, in Windfall's panel.
    User,
    /// An agent, which may only when the user allowed it.
    Agent(&'a str),
}

#[derive(Default)]
struct State {
    next: u64,
    entries: VecDeque<Entry>,
    trusted: BTreeSet<String>,
    agents: VecDeque<String>,
}

#[derive(Default)]
pub struct Approvals {
    state: Mutex<State>,
    changed: Condvar,
}

impl Approvals {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Files a proposal and returns its id.
    pub fn submit(
        &self,
        agent: &str,
        proposal: CommandProposal,
        preview: Preview,
    ) -> Result<String, String> {
        let mut state = self.state();
        let waiting = state
            .entries
            .iter()
            .filter(|entry| !entry.status.decided())
            .count();
        if waiting >= MAX_WAITING {
            return Err(format!(
                "{MAX_WAITING} proposals are already waiting for the user; wait for a decision or withdraw one first"
            ));
        }
        state.next += 1;
        let id = format!("p{}", state.next);
        state.entries.push_back(Entry {
            id: id.clone(),
            agent: agent.to_owned(),
            proposal,
            preview,
            status: Status::Pending,
            automatic: false,
            filed: Instant::now(),
        });
        while state.entries.len() > MAX_KEPT {
            match state
                .entries
                .iter()
                .position(|entry| entry.status.decided())
            {
                Some(index) => {
                    state.entries.remove(index);
                }
                None => break,
            }
        }
        drop(state);
        self.changed.notify_all();
        Ok(id)
    }

    /// Every proposal kept, oldest first.
    pub fn entries(&self) -> Vec<Entry> {
        self.state().entries.iter().cloned().collect()
    }

    /// Whether a proposal waits for the user.
    pub fn waiting(&self) -> bool {
        self.state()
            .entries
            .iter()
            .any(|entry| entry.status == Status::Pending)
    }

    pub fn get(&self, id: &str) -> Option<Entry> {
        self.state()
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    /// Marks a pending proposal as being applied and hands it out.
    pub fn begin(&self, id: &str, approver: Approver<'_>) -> Result<CommandProposal, String> {
        let mut state = self.state();
        let trusted = match approver {
            Approver::User => true,
            Approver::Agent(agent) => state.trusted.contains(agent),
        };
        let entry = state
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("there is no proposal {id}"))?;
        if let Approver::Agent(agent) = approver {
            if entry.agent != agent {
                return Err(format!("proposal {id} belongs to another agent"));
            }
            if !trusted {
                return Err(format!(
                    "proposal {id} is waiting for the user to apply it in Windfall; this agent may not apply without asking"
                ));
            }
        }
        if entry.status != Status::Pending {
            return Err(format!(
                "proposal {id} is {}, not pending",
                entry.status.name()
            ));
        }
        entry.status = Status::Applying;
        entry.automatic = matches!(approver, Approver::Agent(_));
        Ok(entry.proposal.clone())
    }

    /// Records how an apply that [`begin`](Self::begin) started ended.
    pub fn finish(&self, id: &str, result: Result<u64, String>) -> Status {
        let mut state = self.state();
        let status = match result {
            Ok(revision) => Status::Applied { revision },
            Err(error) => Status::Failed { error },
        };
        if let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) {
            entry.status = status.clone();
        }
        drop(state);
        self.changed.notify_all();
        status
    }

    /// Applies a pending proposal through the host: one undo step.
    pub fn apply(
        &self,
        host: &dyn Host,
        id: &str,
        approver: Approver<'_>,
    ) -> Result<Status, String> {
        let proposal = self.begin(id, approver)?;
        host.proposals_changed();
        let status = self.finish(id, host.apply(proposal));
        host.proposals_changed();
        Ok(status)
    }

    pub fn reject(&self, id: &str) -> Result<(), String> {
        self.decide(id, None, Status::Rejected)
    }

    pub fn withdraw(&self, id: &str, agent: &str) -> Result<(), String> {
        self.decide(id, Some(agent), Status::Withdrawn)
    }

    fn decide(&self, id: &str, agent: Option<&str>, status: Status) -> Result<(), String> {
        let mut state = self.state();
        let entry = state
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("there is no proposal {id}"))?;
        if agent.is_some_and(|agent| entry.agent != agent) {
            return Err(format!("proposal {id} belongs to another agent"));
        }
        if entry.status != Status::Pending {
            return Err(format!(
                "proposal {id} is {}, not pending",
                entry.status.name()
            ));
        }
        entry.status = status;
        drop(state);
        self.changed.notify_all();
        Ok(())
    }

    /// Removes a decided proposal from the list the panel shows.
    pub fn dismiss(&self, id: &str) {
        let mut state = self.state();
        state
            .entries
            .retain(|entry| entry.id != id || !entry.status.decided());
    }

    /// Waits until the proposal is decided, or `timeout` has passed.
    pub fn wait(&self, id: &str, timeout: Duration) -> Option<Entry> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state();
        loop {
            let entry = state.entries.iter().find(|entry| entry.id == id)?;
            if entry.status.decided() {
                return Some(entry.clone());
            }
            let left = deadline.checked_duration_since(Instant::now());
            let Some(left) = left.filter(|left| !left.is_zero()) else {
                return Some(entry.clone());
            };
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// Lets an agent apply its own proposals without asking, or stops it.
    pub fn set_trusted(&self, agent: &str, trusted: bool) {
        let mut state = self.state();
        if trusted {
            state.trusted.insert(agent.to_owned());
        } else {
            state.trusted.remove(agent);
        }
        drop(state);
        self.changed.notify_all();
    }

    pub fn trusted(&self, agent: &str) -> bool {
        self.state().trusted.contains(agent)
    }

    pub fn trusted_agents(&self) -> Vec<String> {
        self.state().trusted.iter().cloned().collect()
    }

    /// Remembers an agent that called a tool, for the panel.
    pub fn seen(&self, agent: &str) {
        let mut state = self.state();
        if state.agents.iter().any(|known| known == agent) {
            return;
        }
        state.agents.push_back(agent.to_owned());
        while state.agents.len() > MAX_AGENTS {
            state.agents.pop_front();
        }
    }

    pub fn agents(&self) -> Vec<String> {
        self.state().agents.iter().cloned().collect()
    }

    /// The server was turned off: what waits is withdrawn and every
    /// allowance ends.
    pub fn clear(&self) {
        let mut state = self.state();
        for entry in &mut state.entries {
            if entry.status == Status::Pending {
                entry.status = Status::Withdrawn;
            }
        }
        state.trusted.clear();
        state.agents.clear();
        drop(state);
        self.changed.notify_all();
    }
}
