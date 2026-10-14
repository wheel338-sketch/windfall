use std::io;

use symphonia::core::errors::Error as SymphoniaError;

/// Why an audio file could not be read or written. The text of each is a
/// coded message (see `windfall_core::notice`) to be shown to the user; a
/// reason it carries is one of Windfall's own words (a term), a coded
/// message itself, or what the operating system or decoder said.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodecError {
    /// The operating system refused to open, read, write or rename the file.
    #[error("{}", windfall_core::notice!("codec.io", error = _0.to_string()))]
    Io(#[from] io::Error),

    /// The data is not in a container or codec Windfall can read, or it
    /// states more channels or a higher sample rate than any real file has.
    #[error("{}", windfall_core::notice!("codec.unsupported", reason = windfall_core::notice::term(_0)))]
    UnsupportedFormat(String),

    /// The file looks like audio but is damaged or cut short, and nothing
    /// usable could be decoded from it.
    #[error("{}", windfall_core::notice!("codec.corrupt", reason = windfall_core::notice::term(_0)))]
    Corrupt(String),

    /// The file is empty or holds no audio frames.
    #[error("{}", windfall_core::notice!("codec.noAudio"))]
    NoAudio,

    /// Decoding would need more memory than [`DecodeOptions`] allows.
    ///
    /// [`DecodeOptions`]: crate::DecodeOptions
    #[error("{}", windfall_core::notice!("codec.tooLarge", megabytes = limit_bytes / (1024 * 1024)))]
    TooLarge {
        /// The limit that was exceeded, in bytes of decoded samples.
        limit_bytes: u64,
    },

    /// The system could not provide the memory the decoded audio needs.
    #[error("{}", windfall_core::notice!("codec.outOfMemory"))]
    OutOfMemory,

    /// A WAV file cannot hold more than 4 GB.
    #[error("{}", windfall_core::notice!("codec.wavTooLarge"))]
    WavTooLarge,

    /// The caller asked for something that cannot be written, such as a file
    /// with no channels.
    #[error("{}", windfall_core::notice!("codec.invalidInput", reason = windfall_core::notice::term(_0)))]
    InvalidInput(String),

    /// An encoder gave up on audio it was handed.
    #[error("{}", windfall_core::notice!("codec.encoding", reason = windfall_core::notice::term(_0)))]
    Encoding(String),
}

/// The reason given when a file's codec has no decoder built in.
pub(crate) const UNREADABLE_CODEC: &str = "it uses a codec Windfall cannot read";

/// Translates a Symphonia error into the matching Windfall error.
pub(crate) fn from_symphonia(error: SymphoniaError) -> CodecError {
    match error {
        SymphoniaError::IoError(io) if io.kind() == io::ErrorKind::UnexpectedEof => {
            CodecError::Corrupt("the file ends unexpectedly".to_owned())
        }
        // The guard can only stop a reader by failing its read, and puts the
        // refusal to report inside the I/O error.
        SymphoniaError::IoError(io) => io.downcast().unwrap_or_else(CodecError::Io),
        SymphoniaError::Unsupported(what) => CodecError::UnsupportedFormat(
            // The two refusals a user is likely to meet get plain wording.
            // Any other keeps Symphonia's own.
            match what {
                "core (probe): no suitable format reader found" => "the format was not recognized",
                "core (codec): unsupported audio codec" => UNREADABLE_CODEC,
                other => other,
            }
            .to_owned(),
        ),
        SymphoniaError::DecodeError(what) | SymphoniaError::LimitError(what) => {
            CodecError::Corrupt(what.to_owned())
        }
        other => CodecError::Corrupt(other.to_string()),
    }
}
