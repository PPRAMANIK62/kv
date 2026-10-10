use std::{
    collections::HashMap,
    fs::{File, OpenOptions, create_dir_all},
    io::{BufRead, BufReader, Error, ErrorKind, Result, Seek, SeekFrom, Write},
    path::Path,
};

/// The name of the llog file inside a store's directory.
const LOG_FILE: &str = "data.log";

/// One entry in the log.
#[derive(Debug, Clone, PartialEq)]
enum Record {
    Set { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

impl Record {
    /// The record's bytes, exactly as they go into the log:
    /// `S <key len> <vallue len>\n<key>\n<value>\n` or
    /// `D <key len>\n<key>\n`.
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Record::Set { key, value } => {
                let header = format!("S {} {}\n", key.len(), value.len());
                out.extend_from_slice(header.as_bytes());
                out.extend_from_slice(key);
                out.push(b'\n');
                out.extend_from_slice(value);
                out.push(b'\n');
            }
            Record::Delete { key } => {
                let header = format!("D {}\n", key.len());
                out.extend_from_slice(header.as_bytes());
                out.extend_from_slice(key);
                out.push(b'\n');
            }
        }
        out
    }
}

/// Reads the next record from `reader`. Returns `Ok(None)` at the end of the log,
/// and otherwise the record and it's length in bytes.
fn read_record(reader: &mut impl BufRead) -> Result<Option<(Record, u64)>> {
    let mut header = Vec::new();

    reader.read_until(b'\n', &mut header)?;
    if header.is_empty() {
        return Ok(None); // a clean end: no bytes left at all
    }

    let mut len = header.len() as u64;
    if header.pop() != Some(b'\n') {
        return Err(corrupt("the log ends inside a record header"));
    }

    let Ok(header_text) = str::from_utf8(&header) else {
        return Err(corrupt("the record header is not text"));
    };

    let fields: Vec<&str> = header_text.split(' ').collect();
    let record = match fields.as_slice() {
        ["S", key_len, value_len] => {
            let key = read_field(reader, parse_len(key_len)?)?;
            let value = read_field(reader, parse_len(value_len)?)?;

            len += key.len() as u64 + 1 + value.len() as u64 + 1;
            Record::Set { key, value }
        }
        ["D", key_len] => {
            let key = read_field(reader, parse_len(key_len)?)?;

            len += key.len() as u64 + 1;
            Record::Delete { key }
        }
        _ => return Err(corrupt("unknown record header")),
    };

    Ok(Some((record, len)))
}

/// Reads `len` bytes followed by the newline that ends every field.
fn read_field(reader: &mut impl BufRead, len: usize) -> Result<Vec<u8>> {
    let mut field = vec![0; len + 1];
    reader.read_exact(&mut field)?;
    if field.pop() != Some(b'\n') {
        return Err(corrupt("a field is not followed by a newline"));
    }
    Ok(field)
}

fn parse_len(text: &str) -> Result<usize> {
    text.parse()
        .map_err(|_| corrupt("a length in a record header is not a number"))
}

/// The error for bytes in the log that don't form a valid record.
fn corrupt(message: &str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

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
    /// Opens the file and starts with an empty index.
    pub fn open(dir: impl AsRef<Path>) -> Result<Store> {
        let dir = dir.as_ref();
        create_dir_all(dir)?;

        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(dir.join(LOG_FILE))?;
        let end = file.metadata()?.len();

        Ok(Store {
            file,
            index: HashMap::new(), // empty
            end,
        })
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
        let bytes = record.encode();
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
        let mut reader = BufReader::new(&mut self.file);
        match read_record(&mut reader)? {
            Some((Record::Set { key: found, value }, _)) if found == key => Ok(Some(value)),
            _ => Err(corrupt("the index points at the wrong record")),
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

/// A fresh, empty directory for one test, under the system's temp dir.
#[cfg(test)]
fn test_dir(name: &str) -> std::path::PathBuf {
    use std::fs;

    let dir = std::env::temp_dir().join(format!("kvkit-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[cfg(test)]
mod store_tests {
    use std::fs;

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
