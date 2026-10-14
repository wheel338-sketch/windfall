//! What can go wrong between the host and a plugin.

use crate::state::InvalidState;

/// A plugin could not be loaded, created, activated or restored. The text
/// is for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginError {
    /// The file could not be loaded as a plugin of its format.
    #[error("{}", windfall_core::notice!("pluginHost.error.load", detail = windfall_core::notice::term(_0.to_string())))]
    Load(String),
    /// The file holds no plugin with the id asked for.
    #[error("{}", windfall_core::notice!("pluginHost.error.notFound", detail = windfall_core::notice::term(_0.to_string())))]
    NotFound(String),
    /// The plugin refused to be created.
    #[error("{}", windfall_core::notice!("pluginHost.error.create", detail = windfall_core::notice::term(_0.to_string())))]
    Create(String),
    /// The plugin describes ports the host cannot feed.
    #[error("{}", windfall_core::notice!("pluginHost.error.layout", detail = windfall_core::notice::term(_0.to_string())))]
    Layout(String),
    /// The plugin refused to be activated.
    #[error("{}", windfall_core::notice!("pluginHost.error.activate", detail = windfall_core::notice::term(_0.to_string())))]
    Activate(String),
    /// Native lifecycle refused to establish an inactive processor.
    #[error("{}", windfall_core::notice!("pluginHost.error.deactivate", detail = windfall_core::notice::term(_0.to_string())))]
    Deactivate(String),
    /// The plugin is active already and has a processor out.
    #[error("{}", windfall_core::notice!("pluginHost.error.alreadyActive"))]
    AlreadyActive,
    /// The plugin could not save or load its state.
    #[error("{}", windfall_core::notice!("pluginHost.error.state", detail = windfall_core::notice::term(_0.to_string())))]
    State(&'static str),
    /// The saved state is not one this host wrote.
    #[error(transparent)]
    InvalidState(#[from] InvalidState),
    /// The host cannot run plugins of this format in this build.
    #[error("{}", windfall_core::notice!("pluginHost.error.unsupported", detail = windfall_core::notice::term(_0.to_string())))]
    Unsupported(&'static str),
}

/// A refused lifecycle transition retains the exact audio half for recovery.
pub struct DeactivationError<T> {
    pub error: PluginError,
    pub returned: T,
}
impl<T> std::fmt::Debug for DeactivationError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl<T> std::fmt::Display for DeactivationError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
