//! Operations on a store, and a generator of random sequences of them.

use std::fmt;

use crate::testkit::rng::Rng;

/// One step of a test: a call on the store's API, or a restart.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Set {
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        key: Vec<u8>,
    },
    Get {
        key: Vec<u8>,
    },
    /// Close the store and open it again from it's directory.
    Reopen,
}

impl Op {
    pub fn set(key: &[u8], value: &[u8]) -> Op {
        Op::Set {
            key: key.to_vec(),
            value: value.to_vec(),
        }
    }

    pub fn delete(key: &[u8]) -> Op {
        Op::Delete { key: key.to_vec() }
    }

    pub fn get(key: &[u8]) -> Op {
        Op::Get { key: key.to_vec() }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Set { key, value } => write!(f, "Op::set({}, {})", bstr(key), bstr(value)),
            Op::Delete { key } => write!(f, "Op::delete({})", bstr(key)),
            Op::Get { key } => write!(f, "Op::get({})", bstr(key)),
            Op::Reopen => write!(f, "Op::Reopen"),
        }
    }
}

/// `bytes` as a Rust byte string literal.
pub(crate) fn bstr(bytes: &[u8]) -> String {
    format!("b\"{}\"", bytes.escape_ascii())
}

/// What random operations look like.
#[derive(Debug, Clone)]
pub struct OpGen {
    /// The bytes, keys are made of.
    pub alphabet: Vec<u8>,
    /// The shortest and longest keys.
    pub key_len: (usize, usize),
    /// The longest value. Values are any bytes, and can be empty.
    pub max_value_len: usize,
    /// How often each kind of operation comes up, relative to the others:
    /// set, delete, get and reopen.
    pub weights: [u64; 4],
}

impl Default for OpGen {
    /// Keys of 0 to 3 letters from "ab", 15 keys in all, so operations
    /// keep landing on the same keys. Values of upto 8 bytes.
    fn default() -> Self {
        OpGen {
            alphabet: b"ab".to_vec(),
            key_len: (0, 3),
            max_value_len: 8,
            weights: [50, 15, 34, 1],
        }
    }
}

impl OpGen {
    /// `steps` operations that depend only on `seed` and on `self`.
    pub fn ops(&self, seed: u64, steps: usize) -> Vec<Op> {
        let mut rng = Rng::new(seed);
        let total: u64 = self.weights.iter().sum();
        let mut ops = Vec::with_capacity(steps);
        for _ in 0..steps {
            let mut roll = rng.below(total);
            let mut kind = 0;
            while roll >= self.weights[kind] {
                roll -= self.weights[kind];
                kind += 1;
            }
            let key = self.key(&mut rng);
            ops.push(match kind {
                0 => {
                    let len = rng.between(0, self.max_value_len);
                    Op::Set {
                        key,
                        value: rng.bytes(len),
                    }
                }
                1 => Op::Delete { key },
                2 => Op::Get { key },
                _ => Op::Reopen,
            });
        }
        ops
    }

    fn key(&self, rng: &mut Rng) -> Vec<u8> {
        let len = rng.between(self.key_len.0, self.key_len.1);
        let mut key = Vec::with_capacity(len);
        for _ in 0..len {
            key.push(*rng.pick(&self.alphabet));
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_ops() {
        let generator = OpGen::default();
        assert_eq!(generator.ops(3, 500), generator.ops(3, 500));
        assert_ne!(generator.ops(3, 500), generator.ops(4, 500));
    }

    #[test]
    fn ops_print_as_rust() {
        assert_eq!(
            Op::set(b"a\"b", b"\x00\n").to_string(),
            r#"Op::set(b"a\"b", b"\x00\n")"#
        );
        assert_eq!(Op::delete(b"").to_string(), r#"Op::delete(b"")"#);
        assert_eq!(Op::Reopen.to_string(), "Op::Reopen");
    }

    #[test]
    fn the_generator_reaches_the_corners() {
        // A generator that never makes a case can't find its bugs. Check
        // that a short run has every kind of operation, the empty key, the
        // empty value, and a set of the empty key to the empty value.
        let ops = OpGen::default().ops(0, 5000);
        let has = |want: fn(&Op) -> bool| ops.iter().any(want);
        assert!(has(|op| matches!(op, Op::Set { .. })));
        assert!(has(|op| matches!(op, Op::Delete { .. })));
        assert!(has(|op| matches!(op, Op::Get { .. })));
        assert!(has(|op| matches!(op, Op::Reopen)));
        assert!(has(|op| matches!(op, Op::Delete { key } if key.is_empty())));
        assert!(has(
            |op| matches!(op, Op::Set { value, .. } if value.is_empty())
        ));
        assert!(has(|op| *op == Op::set(b"", b"")));
        let longest = |op: &Op| match op {
            Op::Set { key, .. } => key.len(),
            _ => 0,
        };
        assert_eq!(ops.iter().map(longest).max(), Some(3));
    }
}
