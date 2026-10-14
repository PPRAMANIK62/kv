//! The bytes of one record in the log.
//!
//! ```text
//! offset 0  crc32c    u32, little-endian, of every byte from offset 4 on
//! offset 4  body_len  u32, little-endian, the number of bytes after offset 9
//! offset 8  kind      u8, 1 = set, 2 = delete
//! offset 9  body      key length (varint), key, value (sets only)
//! ```

use std::io::Read;

use crate::{
    KvError, Result,
    checksum::crc32c,
    codec::{Decoder, Encoder},
};

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

impl Record {
    /// The record's bytes, exactly as they go into the log.
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        let (kind, key, value) = match self {
            Record::Set { key, value } => (KIND_SET, key, value.as_slice()),
            Record::Delete { key } => (KIND_DELETE, key, [].as_slice()),
        };
        if key.len() > MAX_KEY_LEN {
            return Err(KvError::KeyTooLarge { len: key.len() });
        }
        if value.len() > MAX_VALUE_LEN {
            return Err(KvError::ValueTooLarge { len: value.len() });
        }
        let mut encoder = Encoder::new();
        encoder.put_u32(0); // the checksum, filled in below
        encoder.put_u32(0); // the body len, filled in below
        encoder.put_u8(kind);
        encoder.put_len_prefixed(key);
        encoder.put_bytes(value);
        let mut bytes = encoder.finish();

        // The limits above keep the body far below 4 GiB, so it fits in u32.
        let body_len = (bytes.len() - HEADER_LEN) as u32;
        bytes[4..8].copy_from_slice(&body_len.to_le_bytes());
        let crc = crc32c(&bytes[4..]);
        bytes[0..4].copy_from_slice(&crc.to_le_bytes());
        Ok(bytes)
    }

    /// Decodes `bytes`, which must hold exactly one whole record.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Record, &'static str> {
        let short = |_| "the record is shorter than a header";
        let mut decoder = Decoder::new(bytes);
        let crc = decoder.get_u32().map_err(short)?;
        let body_len = decoder.get_u32().map_err(short)?;
        let kind = decoder.get_u8().map_err(short)?;
        if body_len as usize != decoder.remaining() {
            return Err("the body length does not match the record");
        }
        if crc32c(&bytes[4..]) != crc {
            return Err("checksum mismatch");
        }
        let Ok(key) = decoder.get_len_prefixed() else {
            return Err("the key is longer than the body");
        };
        let value = decoder.rest();
        match kind {
            KIND_SET => Ok(Record::Set {
                key: key.to_vec(),
                value: value.to_vec(),
            }),
            KIND_DELETE if value.is_empty() => Ok(Record::Delete { key: key.to_vec() }),
            KIND_DELETE => Err("a delete record has a value"),
            _ => Err("unknown record kind"),
        }
    }
}

/// Reads the record that starts at `offset` from `reader`, which must be
/// positioned there. `end` is the length of the log. Returns the record
/// and it's length in bytes.
pub(crate) fn read_record(reader: &mut impl Read, offset: u64, end: u64) -> Result<(Record, u64)> {
    let left = end - offset;
    if left < HEADER_LEN as u64 {
        return Err(KvError::Truncated { offset });
    }
    let mut header = [0; HEADER_LEN];
    reader.read_exact(&mut header)?;

    // Check the length before trusting it with an allocation.
    let body_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if body_len as usize > MAX_BODY_LEN {
        let reason = "the length is larger than any record we write";
        return Err(KvError::Corrupt { offset, reason });
    }
    let len = HEADER_LEN as u64 + u64::from(body_len);
    if len > left {
        return Err(KvError::Truncated { offset });
    }

    let mut bytes = vec![0; len as usize];
    bytes[..HEADER_LEN].copy_from_slice(&header);
    reader.read_exact(&mut bytes[HEADER_LEN..])?;
    match Record::decode(&bytes) {
        Ok(record) => Ok((record, len)),
        Err(reason) => Err(KvError::Corrupt { offset, reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(key: &[u8], value: &[u8]) -> Record {
        Record::Set {
            key: key.to_vec(),
            value: value.to_vec(),
        }
    }

    #[test]
    fn a_set_record_has_the_documented_bytes() {
        let bytes = set(b"apple", b"red").encode().unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(&0x1765_AC6B_u32.to_le_bytes()); // checksum
        expected.extend_from_slice(&[9, 0, 0, 0]); // body length
        expected.push(1); // kind: set
        expected.push(5); // key length
        expected.extend_from_slice(b"applered");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn records_round_trip() {
        let records = [
            set(b"apple", b"red"),
            set(b"", b""),
            set(b"\x00\xff", &[7; 1000]),
            Record::Delete {
                key: b"apple".to_vec(),
            },
        ];
        for record in records {
            let bytes = record.encode().unwrap();
            assert_eq!(Record::decode(&bytes), Ok(record.clone()));
            let (read, len) = read_record(&mut bytes.as_slice(), 0, bytes.len() as u64).unwrap();
            assert_eq!(read, record);
            assert_eq!(len, bytes.len() as u64);
        }
    }

    #[test]
    fn oversized_keys_and_values_are_refused() {
        let key = vec![0; MAX_KEY_LEN + 1];
        let err = set(&key, b"").encode().unwrap_err();
        assert!(matches!(err, KvError::KeyTooLarge { len } if len == MAX_KEY_LEN + 1));
        let value = vec![0; MAX_VALUE_LEN + 1];
        let err = set(b"k", &value).encode().unwrap_err();
        assert!(matches!(err, KvError::ValueTooLarge { .. }));
        // The largest allowed key still decodes.
        let record = set(&key[1..], b"v");
        assert_eq!(Record::decode(&record.encode().unwrap()), Ok(record));
    }
}

#[cfg(test)]
mod damage_tests {
    use super::*;

    #[test]
    fn every_cut_short_record_is_an_error() {
        let bytes = set_apple_red();
        for len in 0..bytes.len() {
            assert!(Record::decode(&bytes[..len]).is_err(), "{len} bytes");
            let err = read_record(&mut &bytes[..len], 0, len as u64).unwrap_err();
            assert!(
                matches!(err, KvError::Truncated { offset: 0 }),
                "{len}: {err}"
            );
        }
    }

    #[test]
    fn every_one_and_two_bit_flip_is_caught() {
        let bytes = set_apple_red();
        let bits = bytes.len() * 8;
        for i in 0..bits {
            for j in i..bits {
                let mut damaged = bytes.clone();
                damaged[i / 8] ^= 1 << (i % 8);
                if j != i {
                    damaged[j / 8] ^= 1 << (j % 8);
                }
                assert!(Record::decode(&damaged).is_err(), "bits {i} and {j}");
            }
        }
    }

    #[test]
    fn random_bytes_are_never_a_record() {
        for seed in 0..10_000 {
            let mut bytes = crate::test_bytes(seed, (seed % 64) as usize);
            let len = bytes.len() as u64;
            assert!(Record::decode(&bytes).is_err(), "seed {seed}");
            assert!(read_record(&mut bytes.as_slice(), 0, len).is_err());
            // Give the bytes a body length that fits, so that only the
            // checksum stands in the way.
            if bytes.len() >= HEADER_LEN {
                let body_len = (bytes.len() - HEADER_LEN) as u32;
                bytes[4..8].copy_from_slice(&body_len.to_le_bytes());
                let result = Record::decode(&bytes);
                assert_eq!(result, Err("checksum mismatch"), "seed {seed}");
            }
        }
    }

    fn set_apple_red() -> Vec<u8> {
        let record = Record::Set {
            key: b"apple".to_vec(),
            value: b"red".to_vec(),
        };
        record.encode().unwrap()
    }
}

#[cfg(test)]
mod oracle_tests {
    use super::*;

    /// A record whose kind, key and value depend only on `seed`.
    fn random_record(seed: u64) -> Record {
        let bytes = crate::test_bytes(seed, 256);
        let key_len = usize::from(bytes[0] % 32);
        let value_len = usize::from(bytes[1]) % 200;
        let key = bytes[2..2 + key_len].to_vec();
        let value = bytes[2 + key_len..2 + key_len + value_len].to_vec();
        if bytes[0] & 0x80 == 0 {
            Record::Set { key, value }
        } else {
            Record::Delete { key }
        }
    }

    /// Encodes `count` random records, checks every checksum against the
    /// `crc32c` crate, and decodes them all back.
    fn check_records(count: u64) {
        let mut log = Vec::new();
        for seed in 0..count {
            let bytes = random_record(seed).encode().unwrap();
            let stored = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            assert_eq!(stored, ::crc32c::crc32c(&bytes[4..]), "seed {seed}");
            log.extend_from_slice(&bytes);
        }
        let end = log.len() as u64;
        let mut reader = log.as_slice();
        let mut offset = 0;
        for seed in 0..count {
            let (record, len) = read_record(&mut reader, offset, end).unwrap();
            assert_eq!(record, random_record(seed), "seed {seed}");
            offset += len;
        }
        assert_eq!(offset, end);
    }

    #[test]
    fn ten_thousand_records_match_the_oracle() {
        check_records(10_000);
    }

    /// Run it with `cargo test --release -p kvkit a_million -- --ignored`.
    #[test]
    #[ignore]
    fn a_million_records_match_the_oracle() {
        check_records(1_000_000);
    }
}
