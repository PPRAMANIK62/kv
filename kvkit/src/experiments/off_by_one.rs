use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

use crate::{
    Result, Store,
    record::{HEADER_LEN, Record, read_record},
    testkit::Engine,
};

/// The real `Store`, but opened with the planted `replay` below.
pub struct Planted(Store);

impl Engine for Planted {
    fn open(dir: &Path) -> Result<Planted> {
        Store::open_with(dir, replay).map(Planted)
    }
    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.0.set(key, value)
    }
    fn delete(&mut self, key: &[u8]) -> Result<bool> {
        self.0.delete(key)
    }
    fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.0.get(key)
    }
    fn scan_all(&mut self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.0.scan_all()
    }
}

/// The shortest record there is: a header and a one-byte key length,
/// for an empty key and no value.
const MIN_RECORD_LEN: u64 = HEADER_LEN as u64 + 1;

/// Reads the log, `end` bytes long, from the start and returns the index it
/// describes.
fn replay(file: &mut File, end: u64) -> Result<HashMap<Vec<u8>, u64>> {
    let mut index = HashMap::new();
    let mut reader = BufReader::new(file);
    let mut offset = 0;
    // Stop when what is left is also too short to hold even the smallest record.
    while offset + MIN_RECORD_LEN < end {
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

#[cfg(test)]
mod model_tests {
    use std::fs;

    use super::Planted;
    use crate::testkit::{Op, OpGen, check, model_test, scratch_dir};

    #[test]
    fn the_traced_sequence_fails_at_step_3() {
        let dir = scratch_dir("steps_traced");
        let ops = [
            Op::set(b"a", b"x"),
            Op::set(b"", b""),
            Op::Reopen,
            Op::get(b""),
        ];
        let failure = check::<Planted>(&dir, &ops).unwrap_err();
        println!("{failure}");
        assert_eq!(failure.step, 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[should_panic(expected = "minimized from 5000 to 3 steps")]
    fn the_model_test_finds_it() {
        model_test::<Planted>("steps_model", &OpGen::default(), 0..20, 5000);
    }

    /// Keys of 1 to 3 letters: no empty key, so never the 10-byte record.
    #[test]
    fn a_generator_without_empty_keys_misses_it() {
        let generator = OpGen {
            key_len: (1, 3),
            ..OpGen::default()
        };
        model_test::<Planted>("steps_model_no_empty", &generator, 0..20, 5000);
    }
}
