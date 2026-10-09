/// Logical sequence number of a WAL record.
/// LSNs identify logical order in the log. They are intentionally
/// separate from physical byte offsets because segment rotation
/// changes physical location without changing logical ordering.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Lsn(u64);

/// Stable identifier for a WAL segment index.
///
/// Wraps a raw u64 segment number so callers cannot confuse it with
/// an LSN, file offset, or any other u64 quantity in the WAL.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct SegmentId(u64);

/// Byte offset within a WAL segment.
///
/// Wraps a raw u64 byte offset so callers cannot confuse it with
/// an LSN, segment number, or any other u64 quantity in the WAL.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct FileOffset(u64);

impl Lsn {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// Return the next sequential LSN.
    ///
    /// Storage identity must never silently wrap, so exhaustion is a typed
    /// error instead of a panic; use [`Lsn::checked_next`] for Option-based
    /// handling. (not const: `Option::ok_or` is not const-stable.)
    pub fn next(&self) -> Result<Self, crate::error::Error> {
        self.checked_next().ok_or(crate::error::Error::LsnExhausted)
    }

    /// Return the next sequential LSN, or `None` at `u64::MAX`.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

impl From<u64> for Lsn {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Lsn> for u64 {
    fn from(value: Lsn) -> Self {
        value.0
    }
}

/// Physical location within a segment.
///
/// Implementation detail: consumers append through `Wal`, they do not
/// construct positions.
///
/// `ponytail:` `SegmentId` and `FileOffset` are pub(crate), not exposed
/// publicly — this is M1 storage-model hygiene, not API surface. If the
/// external API ever grows a position type it should carry a fresh set
/// of typed wrappers.
pub(crate) struct Position {
    pub(crate) segment: SegmentId,
    pub(crate) offset: FileOffset,
}

impl Position {
    pub(crate) fn new(segment: u64, offset: u64) -> Self {
        Self {
            segment: SegmentId(segment),
            offset: FileOffset(offset),
        }
    }
}

impl SegmentId {
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

impl FileOffset {
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}
