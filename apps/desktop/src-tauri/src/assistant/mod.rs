//! Explicit-action BYOK service. No project persistence, SDK, automatic requests or logs.
mod connection;
mod provider;
use connection::{load, save};
use std::path::Path;
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::task::Waker;
use windfall_project::assistant::{AssistantContext, AssistantProvider, AssistantReply};

#[derive(Default)]
pub struct Service {
    ticket: AtomicU64,
    worker: Mutex<()>,
    configuring: Mutex<()>,
    wake: Mutex<Option<Waker>>,
}
impl Service {
    pub fn begin(&self) -> u64 {
        let next = self.ticket.fetch_add(1, Ordering::AcqRel) + 1;
        self.wake();
        next
    }
    fn wake(&self) {
        let waker = crate::sync::lock(&self.wake).take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    pub fn cancel(&self, request: u64) {
        if self
            .ticket
            .compare_exchange(
                request,
                request.saturating_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.wake();
        }
    }
    pub fn status(&self, file: &Path) -> Result<windfall_ipc::AssistantConnection, String> {
        let _guard = crate::sync::lock(&self.configuring);
        Ok(load(file)?.status())
    }
    pub fn configure(
        &self,
        file: &Path,
        update: windfall_ipc::AssistantConnectionUpdate,
    ) -> Result<windfall_ipc::AssistantConnection, String> {
        let _guard = crate::sync::lock(&self.configuring);
        let mut config = load(file)?;
        config.update(update)?;
        save(file, &config)?;
        self.begin(); // New connection invalidates requests made with the old key/endpoint.
        Ok(config.status())
    }
    pub fn respond(
        &self,
        file: &Path,
        request: u64,
        prompt: &str,
        context: AssistantContext<'_>,
    ) -> Result<AssistantReply, String> {
        let _worker = self
            .worker
            .try_lock()
            .map_err(|_| "an assistant request is already running; cancel and retry")?;
        let config = {
            let _guard = crate::sync::lock(&self.configuring);
            load(file)?
        };
        config.ready()?; // OFF/no key fails without constructing an HTTP client.
        let result = provider::Provider {
            config: &config,
            timeout: std::time::Duration::from_secs(15),
            service: self,
            request,
        }
        .respond(prompt, context);
        crate::sync::lock(&self.wake).take();
        if self.cancelled(request) {
            Err("assistant request cancelled".into())
        } else {
            result
        }
    }
    fn cancelled(&self, request: u64) -> bool {
        self.ticket.load(Ordering::Acquire) != request
    }
}

#[cfg(test)]
pub(crate) mod tests;
