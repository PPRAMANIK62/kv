//! The model test: run operations against a store and against a map,
//! and compare after every step.

use std::{collections::BTreeMap, fmt, fs, path::Path};

use crate::{
    Result, Store,
    testkit::ops::{Op, bstr},
};

pub trait Engine: Sized {
    fn open(dir: &Path) -> Result<Self>;
    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&mut self, key: &[u8]) -> Result<bool>;
    fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn scan_all(&mut self) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
}

impl Engine for Store {
    fn open(dir: &Path) -> Result<Self> {
        Store::open(dir)
    }
    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        Store::set(self, key, value)
    }
    fn delete(&mut self, key: &[u8]) -> Result<bool> {
        Store::delete(self, key)
    }
    fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Store::get(self, key)
    }
    fn scan_all(&mut self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        Store::scan_all(self)
    }
}

/// What an operation returned, or what a store holds.
#[derive(Debug, PartialEq)]
enum Outcome {
    Done,
    Deleted(bool),
    Value(Option<Vec<u8>>),
    Pairs(Vec<(Vec<u8>, Vec<u8>)>),
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Done => write!(f, "Ok(())"),
            Outcome::Deleted(found) => write!(f, "Ok({found})"),
            Outcome::Value(None) => write!(f, "Ok(None)"),
            Outcome::Value(Some(value)) => write!(f, "Ok(Some({}))", bstr(value)),
            Outcome::Pairs(pairs) => {
                let pairs: Vec<String> = pairs
                    .iter()
                    .map(|(key, value)| format!("{}: {}", bstr(key), bstr(value)))
                    .collect();
                write!(f, "{{{}}}", pairs.join(", "))
            }
        }
    }
}

/// The first step at which the store and the model disagreed.
#[derive(Debug)]
pub struct Failure {
    /// The step, counting from 1. `ops[..step]` is enough to fail again.
    pub step: usize,
    /// The operation of that step.
    pub op: Op,
    /// "result" for the operation's return value, "contents" for what
    /// the store holds after it.
    pub what: &'static str,
    /// What the model says.
    pub expected: String,
    /// What the store says.
    pub actual: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "step {}, {}: the {} should be {} but the store gives {}",
            self.step, self.op, self.what, self.expected, self.actual
        )
    }
}

/// Runs `ops` against a new store in `dir` and against the model,
/// comparing each operation's result and then the whole contents after
/// every step. Returns the first difference.
pub fn check<E: Engine>(dir: &Path, ops: &[Op]) -> Result<(), Failure> {
    let _ = fs::remove_dir_all(dir);
    let mut store = E::open(dir).expect("opening a new, empty store");
    let mut model: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

    for (i, op) in ops.iter().enumerate() {
        let fail = |what, expected: &Outcome, actual: String| Failure {
            step: i + 1,
            op: op.clone(),
            what,
            expected: expected.to_string(),
            actual,
        };

        // The model: the dumbest code that is obviously right.
        let expected = match op {
            Op::Set { key, value } => {
                model.insert(key.clone(), value.clone());
                Outcome::Done
            }
            Op::Delete { key } => Outcome::Deleted(model.remove(key).is_some()),
            Op::Get { key } => Outcome::Value(model.get(key).cloned()),
            Op::Reopen => Outcome::Done,
        };

        // The store under test.
        let actual = match op {
            Op::Set { key, value } => store.set(key, value).map(|()| Outcome::Done),
            Op::Delete { key } => store.delete(key).map(Outcome::Deleted),
            Op::Get { key } => store.get(key).map(Outcome::Value),
            Op::Reopen => {
                drop(store);
                match E::open(dir) {
                    Ok(reopened) => {
                        store = reopened;
                        Ok(Outcome::Done)
                    }
                    Err(e) => return Err(fail("result", &expected, format!("Err({e})"))),
                }
            }
        };

        match actual {
            Ok(actual) if actual == expected => {}
            Ok(actual) => return Err(fail("result", &expected, actual.to_string())),
            Err(e) => return Err(fail("result", &expected, format!("Err({e})"))),
        }

        // The whole contents, after every step.
        let expected = Outcome::Pairs(model.clone().into_iter().collect());
        match store.scan_all() {
            Ok(pairs) if Outcome::Pairs(pairs.clone()) == expected => {}
            Ok(pairs) => {
                let actual = Outcome::Pairs(pairs).to_string();
                return Err(fail("contents", &expected, actual));
            }
            Err(e) => return Err(fail("contents", &expected, format!("Err({e})"))),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store that keeps everything in memory, and so forgets it all on
    /// reopen. The model test must notice.
    pub(super) struct Forgetful(BTreeMap<Vec<u8>, Vec<u8>>);

    impl Engine for Forgetful {
        fn open(_dir: &Path) -> Result<Forgetful> {
            Ok(Forgetful(BTreeMap::new()))
        }
        fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
            self.0.insert(key.to_vec(), value.to_vec());
            Ok(())
        }
        fn delete(&mut self, key: &[u8]) -> Result<bool> {
            Ok(self.0.remove(key).is_some())
        }
        fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
            Ok(self.0.get(key).cloned())
        }
        fn scan_all(&mut self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
            Ok(self.0.clone().into_iter().collect())
        }
    }

    /// The sequence the chapter traces by hand.
    fn traced() -> Vec<Op> {
        vec![
            Op::set(b"a", b"x"),
            Op::set(b"", b""),
            Op::Reopen,
            Op::get(b""),
        ]
    }

    #[test]
    fn the_store_passes_the_traced_sequence() {
        let dir = scratch_dir("model_traced");
        check::<Store>(&dir, &traced()).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_store_that_forgets_fails_at_the_reopen() {
        let dir = scratch_dir("model_forgetful");
        let failure = check::<Forgetful>(&dir, &traced()).unwrap_err();
        assert_eq!(failure.step, 3);
        assert_eq!(failure.op, Op::Reopen);
        assert_eq!(failure.what, "contents");
        assert_eq!(failure.expected, r#"{b"": b"", b"a": b"x"}"#);
        assert_eq!(failure.actual, "{}");
    }
}
