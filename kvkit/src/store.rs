use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufReader, Seek, SeekFrom, Write},
    path::Path,
};

use crate::{
    KvError, Result,
    record::{Record, read_record},
};

/// The name of the llog file inside a store's directory.
const LOG_FILE: &str = "data.log";

/// A key-value store kept in a single append-only log file.
pub struct Store {
    /// The log, opened for reading and appending.
    file: File,
    /// For every live key, the offset of it's latest `Set` record.
    index: HashMap<Vec<u8>, u64>,
    /// The length of the log: the offset where the next record goes.
    end: u64,
}

impl Store {
    /// Opens the store in directory `dir`, creating the directory and
    /// an empty log if the don't exist, and rebuilds the index from the log.
    pub fn open(dir: impl AsRef<Path>) -> Result<Store> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir)?;
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(dir.join(LOG_FILE))?;
        let end = file.metadata()?.len();
        let index = replay(&mut file, end)?;
        Ok(Store { file, index, end })
    }

    /// Sets `key` to `value`, replacing any earlier value.
    pub fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        let record = Record::Set {
            key: key.to_vec(),
            value: value.to_vec(),
        };
        let offset = self.append(&record)?;
        self.index.insert(key.to_vec(), offset);
        Ok(())
    }

    /// Deletes `key`. Returns whether the key was there.
    pub fn delete(&mut self, key: &[u8]) -> Result<bool> {
        if !self.index.contains_key(key) {
            return Ok(false);
        }
        self.append(&Record::Delete { key: key.to_vec() })?;
        self.index.remove(key);
        Ok(true)
    }

    /// Writes `record` at the end of the log and returns it's offset.
    fn append(&mut self, record: &Record) -> Result<u64> {
        let bytes = record.encode()?;
        self.file.write_all(&bytes)?;
        let offset = self.end;
        self.end += bytes.len() as u64;
        Ok(offset)
    }

    /// Returns the value of `key` or `None` if it isn't set.
    pub fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let Some(&offset) = self.index.get(key) else {
            return Ok(None);
        };
        self.file.seek(SeekFrom::Start(offset))?;
        match read_record(&mut self.file, offset, self.end)? {
            (Record::Set { key: found, value }, _) if found == key => Ok(Some(value)),
            _ => {
                let reason = "the index points at the wrong record";
                Err(KvError::Corrupt { offset, reason })
            }
        }
    }

    /// Returns every live key and it's value, sorted by key.
    pub fn scan_all(&mut self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut keys = Vec::new();
        for key in self.index.keys() {
            keys.push(key.clone());
        }
        keys.sort();
        let mut pairs = Vec::new();
        for key in keys {
            if let Some(value) = self.get(&key)? {
                pairs.push((key, value));
            }
        }
        Ok(pairs)
    }
}

/// Reads the whole log from the start and returns the index it describes,
/// along with the log's length.
fn replay(file: &mut File, end: u64) -> Result<HashMap<Vec<u8>, u64>> {
    let mut index = HashMap::new();
    let mut reader = BufReader::new(file);
    let mut offset = 0;
    while offset < end {
        let (record, len) = read_record(&mut reader, offset, end)?;
        match record {
            Record::Set { key, .. } => {
                index.insert(key, offset);
            }
            Record::Delete { key } => {
                index.remove(&key);
            }
        }
        offset += len;
    }
    Ok(index)
}

/// A fresh, empty directory for one test, under the system's temp dir.
#[cfg(test)]
fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kvkit-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[cfg(test)]
mod store_tests {
    use super::*;

    #[test]
    fn set_then_get() {
        let dir = test_dir("set_then_get");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"apple", b"red").unwrap();
        store.set(b"banana", b"yellow").unwrap();
        assert_eq!(store.get(b"apple").unwrap(), Some(b"red".to_vec()));
        assert_eq!(store.get(b"banana").unwrap(), Some(b"yellow".to_vec()));
        assert_eq!(store.get(b"cherry").unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn overwrite_returns_the_latest_value() {
        let dir = test_dir("overwrite");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"apple", b"red").unwrap();
        store.set(b"apple", b"green").unwrap();
        assert_eq!(store.get(b"apple").unwrap(), Some(b"green".to_vec()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn delete_removes_the_key() {
        let dir = test_dir("delete");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"apple", b"red").unwrap();
        assert!(store.delete(b"apple").unwrap());
        assert_eq!(store.get(b"apple").unwrap(), None);
        // Deleting a key that isn't there reports it.
        assert!(!store.delete(b"apple").unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keys_and_values_can_hold_any_bytes() {
        let dir = test_dir("any_bytes");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"two\nlines", b"S 1 2\n").unwrap();
        store.set(b"\x00\xff", b"").unwrap();
        assert_eq!(store.get(b"two\nlines").unwrap(), Some(b"S 1 2\n".to_vec()));
        assert_eq!(store.get(b"\x00\xff").unwrap(), Some(Vec::new()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_all_returns_live_pairs_in_key_order() {
        let dir = test_dir("scan_all");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"cherry", b"dark red").unwrap();
        store.set(b"apple", b"red").unwrap();
        store.set(b"banana", b"yellow").unwrap();
        store.delete(b"banana").unwrap();
        store.set(b"apple", b"green").unwrap();
        let expected = vec![
            (b"apple".to_vec(), b"green".to_vec()),
            (b"cherry".to_vec(), b"dark red".to_vec()),
        ];
        assert_eq!(store.scan_all().unwrap(), expected);
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod reopen_tests {
    use super::*;

    #[test]
    fn reopen_rebuilds_the_index() {
        let dir = test_dir("reopen");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"apple", b"red").unwrap();
        store.set(b"banana", b"yellow").unwrap();
        store.set(b"apple", b"green").unwrap();
        store.delete(b"banana").unwrap();
        drop(store);

        let mut store = Store::open(&dir).unwrap();
        assert_eq!(store.get(b"apple").unwrap(), Some(b"green".to_vec()));
        assert_eq!(store.get(b"banana").unwrap(), None);
        // New writes go after the old records, not over them.
        store.set(b"cherry", b"dark red").unwrap();
        drop(store);

        let mut store = Store::open(&dir).unwrap();
        assert_eq!(store.get(b"apple").unwrap(), Some(b"green".to_vec()));
        assert_eq!(store.get(b"cherry").unwrap(), Some(b"dark red".to_vec()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_go_to_the_end_after_a_read() {
        let dir = test_dir("append_after_read");
        let mut store = Store::open(&dir).unwrap();
        // A value bigger than BufReader's 8 KiB buffer, so that reading it
        // leaves the file's cursor in the middle of the log.
        let big = vec![b'x'; 20_000];
        store.set(b"big", &big).unwrap();
        store.set(b"small", b"1").unwrap();
        assert_eq!(store.get(b"big").unwrap(), Some(big.clone()));
        store.set(b"after", b"2").unwrap();
        drop(store);

        let mut store = Store::open(&dir).unwrap();
        assert_eq!(store.get(b"big").unwrap(), Some(big));
        assert_eq!(store.get(b"small").unwrap(), Some(b"1".to_vec()));
        assert_eq!(store.get(b"after").unwrap(), Some(b"2".to_vec()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_write_survives_without_closing_the_store() {
        let dir = test_dir("no_close");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"apple", b"red").unwrap();
        // Skip Store's destructor, as a process killed with SIGKILL would.
        std::mem::forget(store);

        let mut store = Store::open(&dir).unwrap();
        assert_eq!(store.get(b"apple").unwrap(), Some(b"red".to_vec()));
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod damage_tests {
    use crate::KvError;

    use super::*;

    /// A store directory whose log holds `set fig purple` (19 bytes) and
    /// `set fig red` (16 bytes), then has `damage` done to it.
    fn damaged_store(name: &str, damage: impl FnOnce(&mut Vec<u8>)) -> std::path::PathBuf {
        let dir = test_dir(name);
        let mut store = Store::open(&dir).unwrap();
        store.set(b"fig", b"purple").unwrap();
        store.set(b"fig", b"red").unwrap();
        drop(store);
        let mut log = fs::read(dir.join(LOG_FILE)).unwrap();
        damage(&mut log);
        fs::write(dir.join(LOG_FILE), log).unwrap();
        dir
    }

    #[test]
    fn a_torn_last_record_stops_open() {
        // The second record lost its last 3 bytes.
        let dir = damaged_store("torn", |log| log.truncate(log.len() - 3));
        let err = Store::open(&dir).err().unwrap();
        assert!(matches!(err, KvError::Truncated { offset: 19 }), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_kind_stops_open() {
        // The second record's kind, at 19 + 8, became 2: a delete.
        let dir = damaged_store("bad_kind", |log| log[27] = 2);
        let err = Store::open(&dir).err().unwrap();
        assert!(matches!(err, KvError::Corrupt { offset: 19, .. }), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_flipped_bit_in_a_value_stops_open() {
        // "red" is the last 3 bytes. Flip the lowest bit of its "d".
        let dir = damaged_store("flipped_bit", |log| *log.last_mut().unwrap() ^= 1);
        let err = Store::open(&dir).err().unwrap();
        assert!(matches!(err, KvError::Corrupt { offset: 19, .. }), "{err}");
        assert!(err.to_string().ends_with("checksum mismatch"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_huge_length_is_an_error_not_an_allocation() {
        // The first record's body length, at bytes 4..8, became 4 GiB - 1.
        let dir = damaged_store("huge_len", |log| log[4..8].fill(0xff));
        let err = Store::open(&dir).err().unwrap();
        assert!(matches!(err, KvError::Corrupt { offset: 0, .. }), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_length_past_the_end_is_truncated() {
        // The second record's body length, at 19 + 4, grew by 100 bytes.
        let dir = damaged_store("long_len", |log| log[23] += 100);
        let err = Store::open(&dir).err().unwrap();
        assert!(matches!(err, KvError::Truncated { offset: 19 }), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn records_land_at_the_expected_offsets() {
        let dir = test_dir("offsets");
        let mut store = Store::open(&dir).unwrap();
        store.set(b"apple", b"red").unwrap();
        store.set(b"fig", b"purple").unwrap();
        store.delete(b"apple").unwrap();

        let log = fs::read(dir.join(LOG_FILE)).unwrap();
        let apple_red = Record::Set {
            key: b"apple".to_vec(),
            value: b"red".to_vec(),
        };
        let fig_purple = Record::Set {
            key: b"fig".to_vec(),
            value: b"purple".to_vec(),
        };
        let no_apple = Record::Delete {
            key: b"apple".to_vec(),
        };
        let mut expected = apple_red.encode().unwrap();
        expected.extend(fig_purple.encode().unwrap());
        expected.extend(no_apple.encode().unwrap());
        assert_eq!(log, expected);
        // The apple record is 9 + 1 + 5 + 3 = 18 bytes, so fig starts at 18.
        assert_eq!(store.index.get(b"fig".as_slice()), Some(&18));
        assert_eq!(store.index.get(b"apple".as_slice()), None);
        // fig's record is 9 + 1 + 3 + 6 = 19 bytes, the delete 9 + 1 + 5.
        assert_eq!(store.end, 18 + 19 + 15);
        fs::remove_dir_all(&dir).unwrap();
    }
}
