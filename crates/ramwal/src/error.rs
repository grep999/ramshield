use crate::lsn::Lsn;
use std::fmt;

/// A complete record that fails validation.
///
/// Corruption is never repaired automatically.
#[derive(Debug)]
pub enum Corruption {
    InvalidMagic {
        offset: u64,
    },

    UnsupportedVersion {
        offset: u64,
        version: u16,
    },

    InvalidLength {
        offset: u64,
        length: u32,
    },

    InvalidFlags {
        offset: u64,
        flags: u8,
    },

    ChecksumMismatch {
        offset: u64,
        lsn: Lsn,
    },

    /// The LSN is not strictly greater than the previous verified LSN.
    LsnViolation {
        previous: Lsn,
        current: Lsn,
    },

    /// A partial record was found in a segment that is not the final one.
    ///
    /// Only the final segment may end in an incomplete record; a historical
    /// segment with an incomplete tail is damage, not a crash artifact.
    NonFinalSegmentTail {
        offset: u64,
    },

    /// The stored payload passed framing/CRC validation but could not
    /// be decoded according to its declared representation.
    PayloadDecode {
        offset: u64,
        lsn: Lsn,
        reason: String,
    },
}

/// State of the physical tail of a segment.
#[derive(Debug)]
pub enum TailState {
    Clean,

    /// EOF occurred after some, but not all, header bytes.
    PartialHeader {
        offset: u64,
    },

    /// EOF occurred after a complete header but before the complete payload.
    PartialPayload {
        offset: u64,
        expected: u32,
        available: u32,
    },
}

/// Top-level RAMWAL error.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),

    Corruption {
        segment: u64,
        offset: u64,
        reason: Corruption,
    },

    Tail {
        segment: u64,
        state: TailState,
    },

    Manifest(String),

    DiskFull,

    Closed,

/// The WAL encountered a write-side storage failure and must be reopened
/// so recovery can establish a new verified append boundary.
    Poisoned,

    /// The LSN sequence counter exhausted (reached u64::MAX).
    LsnExhausted,

    /// Retention/checkpoint operation attempted to advance beyond
    /// the checkpoint currently recorded by RAMWAL.
    CheckpointRequired {
        requested: Lsn,
        checkpoint: Lsn,
    },

    /// A checkpoint cannot be recorded until the WAL itself is
    /// durable through that LSN.
    CheckpointNotDurable {
        requested: Lsn,
        durable: Lsn,
    },

    /// Checkpoint values are monotonic.
    CheckpointRegression {
        current: Lsn,
        requested: Lsn,
    },

    InvalidConfiguration(String),
}

impl Error {
    pub fn from_io(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::StorageFull {
            Self::DiskFull
        } else {
            Self::Io(e)
        }
    }
}

impl fmt::Display for Corruption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic { offset } => {
                write!(f, "invalid record magic at offset {offset}")
            }
            Self::UnsupportedVersion { offset, version } => {
                write!(f, "unsupported record version {version} at offset {offset}")
            }
            Self::InvalidLength { offset, length } => {
                write!(f, "invalid payload length {length} at offset {offset}")
            }
            Self::InvalidFlags { offset, flags } => {
                write!(f, "invalid record flags 0x{flags:02x} at offset {offset}")
            }
            Self::ChecksumMismatch { offset, lsn } => {
                write!(f, "checksum mismatch at offset {offset} for LSN {lsn:?}")
            }
            Self::LsnViolation { previous, current } => {
                write!(
                    f,
                    "LSN violation: previous={previous:?}, current={current:?}"
                )
            }
            Self::NonFinalSegmentTail { offset } => {
                write!(
                    f,
                    "incomplete record in a non-final segment at offset {offset}"
                )
            }
            Self::PayloadDecode {
                offset,
                lsn,
                reason,
            } => {
                write!(
                    f,
                    "payload decode failure at offset {offset}, LSN {lsn:?}: {reason}"
                )
            }
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),

            Self::Corruption {
                segment,
                offset,
                reason,
            } => {
                write!(
                    f,
                    "corruption in segment {segment} at offset {offset}: {reason}"
                )
            }

            Self::Tail { segment, state } => {
                write!(f, "tail state in segment {segment}: {state:?}")
            }

            Self::Manifest(msg) => write!(f, "manifest error: {msg}"),

            Self::DiskFull => write!(f, "disk full"),

            Self::Closed => write!(f, "WAL is closed"),

            Self::Poisoned => write!(
                f,
                "WAL is poisoned after a storage failure; reopen it for recovery"
            ),

            Self::CheckpointRequired {
                requested,
                checkpoint,
            } => {
                write!(
                    f,
                    "operation requires checkpoint >= {requested:?}, current checkpoint is {checkpoint:?}"
                )
            }

            Self::CheckpointNotDurable { requested, durable } => {
                write!(
                    f,
                    "checkpoint {requested:?} exceeds durable WAL LSN {durable:?}"
                )
            }

            Self::CheckpointRegression { current, requested } => {
                write!(
                    f,
                    "checkpoint regression: current={current:?}, requested={requested:?}"
                )
            }

            Self::InvalidConfiguration(msg) => {
                write!(f, "invalid configuration: {msg}")
            }

            Self::LsnExhausted => write!(f, "LSN sequence exhausted (reached u64::MAX)"),
        }
    }
}

impl std::error::Error for Error {}
impl std::error::Error for Corruption {}

/// Converts std::io::Error into Error::Io.
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::from_io(e)
    }
}
