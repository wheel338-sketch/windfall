use thiserror::Error;

/// Why a command could not be applied. The project is unchanged.
///
/// Its text is a coded message (see `windfall_core::notice`): the reason of
/// an `Invalid` is one already, and a `NotFound` is put into one here.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CommandError {
    #[error("{}", windfall_core::notice!("project.notFound", kind = windfall_core::notice::term(kind), id = id))]
    NotFound { kind: &'static str, id: u32 },
    #[error("{0}")]
    Invalid(String),
}

impl CommandError {
    pub fn not_found(kind: &'static str, id: impl Into<u32>) -> Self {
        Self::NotFound {
            kind,
            id: id.into(),
        }
    }

    pub fn invalid(reason: impl Into<String>) -> Self {
        Self::Invalid(reason.into())
    }
}
