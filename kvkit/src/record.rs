//! The bytes of one record in the log.
//!
//! ```text
//! offset 0  crc32c    u32, little-endian, of every byte from offset 4 on
//! offset 4  body_len  u32, little-endian, the number of bytes after offset 9
//! offset 8  kind      u8, 1 = set, 2 = delete
//! offset 9  body      key length (varint), key, value (sets only)
//! ```

/// The longest key a record can hold.
pub const MAX_KEY_LEN: usize = 65_535;
/// The longest value a record can hold; 64 MiB.
pub const MAX_VALUE_LEN: usize = 64 << 20;

/// The fixed part at the start of every record.
pub(crate) const HEADER_LEN: usize = 9;
/// The longest body we ever write: a 3-byte varint for `MAX_KEY_LEN`,
/// the key and the value.
const MAX_BODY_LEN: usize = 3 + MAX_KEY_LEN + MAX_VALUE_LEN;

const KIND_SET: u8 = 1;
const KIND_DELETE: u8 = 2;

/// One entry in the log.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Record {
    Set { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}
