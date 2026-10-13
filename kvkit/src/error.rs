use std::{error, fmt, io, result};

use crate::record::{MAX_KEY_LEN, MAX_VALUE_LEN};

/// `Result` with `KvError` as it's error type, unless another is given.
pub type Result<T, E = KvError> = result::Result<T, E>;

/// Everything that can go wrong in a `Store`.
#[derive(Debug)]
pub enum KvError {
    /// The operating system reported an error.
    Io(io::Error),
    /// The log ends before the record that starts at `offset` does.
    Truncated { offset: u64 },
    /// The bytes at `offset` are not valid record.
    Corrupt { offset: u64, reason: &'static str },
    /// A key longer than `MAX_KEY_LEN`.
    KeyTooLarge { len: usize },
    /// A value longer than `MAX_VALUE_LEN`.
    ValueTooLarge { len: usize },
}

impl fmt::Display for KvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KvError::Io(e) => write!(f, "I/O error: {e}"),
            KvError::Truncated { offset } => {
                write!(f, "the log ends inside the record at offset {offset}")
            }
            KvError::Corrupt { offset, reason } => {
                write!(f, "bad record at offset {offset}: {reason}")
            }
            KvError::KeyTooLarge { len } => {
                write!(f, "a key of {len} bytes is over the limit of {MAX_KEY_LEN}")
            }
            KvError::ValueTooLarge { len } => {
                write!(
                    f,
                    "a value of {len} bytes is over the limit of {MAX_VALUE_LEN}"
                )
            }
        }
    }
}

impl error::Error for KvError {}

impl From<io::Error> for KvError {
    fn from(value: io::Error) -> Self {
        KvError::Io(value)
    }
}
