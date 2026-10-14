//! Why a file could not be read at all.

use thiserror::Error;

/// A problem that stops a file from being read. Problems that only cost a
/// part of the file are [`Diagnostic`](crate::Diagnostic)s instead, and the
/// rest is still read. Its text is a coded message (see
/// `windfall_core::notice`).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FlpError {
    #[error("{}", windfall_core::notice!("flp.error.notProject"))]
    NotProject,
    #[error("{}", windfall_core::notice!("flp.error.tooLarge", size = size, limit = limit))]
    TooLarge { size: u64, limit: u64 },
    #[error("{}", windfall_core::notice!("flp.error.tooShort", size = size))]
    TooShort { size: usize },
    #[error("{}", windfall_core::notice!("flp.error.zipped"))]
    Zipped,
    #[error("{}", windfall_core::notice!("flp.error.notFlp", found = found))]
    NotFlp { found: String },
    #[error("{}", windfall_core::notice!("flp.error.headerLength", length = length))]
    HeaderLength { length: u32 },
    #[error("{}", windfall_core::notice!("flp.error.zeroPpq"))]
    ZeroPpq,
    #[error("{}", windfall_core::notice!("flp.error.chunkLength", offset = offset, length = length))]
    ChunkLength { offset: usize, length: u32 },
    #[error("{}", windfall_core::notice!("flp.error.noData"))]
    NoData,
}

/// Where an event stream stopped being readable. The events before it are
/// good. Its text is a coded message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EventError {
    #[error("{}", windfall_core::notice!("flp.error.eventTruncated", offset = offset))]
    Truncated { offset: usize },
    #[error("{}", windfall_core::notice!("flp.error.eventLength", offset = offset))]
    Length { offset: usize },
}
