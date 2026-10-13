use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
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
        let (index, end) = replay(&mut file)?;

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

/// Reads the whole log from the start and returns the index it describes,
/// along with the log's length.
fn replay(file: &mut File) -> Result<(HashMap<Vec<u8>, u64>, u64)> {
    let mut index = HashMap::new();
    let mut reader = BufReader::new(file);
    let mut offset = 0;

    loop {
        let (record, len) = match read_record(&mut reader) {
            Ok(Some(found)) => found,
            Ok(None) => break,
            Err(e) => {
                let message = format!("bad record at offset {offset}: {e}");
                return Err(Error::new(e.kind(), message));
            }
        };
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

    Ok((index, offset))
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
    use std::io;

    use super::*;

    /// Writes `log` as the log file of a fresh store directory.
    fn dir_with_log(name: &str, log: &[u8]) -> std::path::PathBuf {
        let dir = test_dir(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(LOG_FILE), log).unwrap();
        dir
    }

    #[test]
    fn a_torn_last_record_stops_open() {
        // The second record lost its last 3 bytes.
        let dir = dir_with_log("torn", b"S 3 6\nfig\npurple\nS 3 3\nfi");
        let err = Store::open(&dir).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        assert!(err.to_string().contains("offset 17"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_header_stops_open() {
        // "S" became "T".
        let dir = dir_with_log("bad_header", b"S 3 6\nfig\npurple\nT 3 3\nfig\nred\n");
        let err = Store::open(&dir).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("offset 17"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
