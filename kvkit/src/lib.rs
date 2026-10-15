//! `kvkit`, the storage library behind `kvctl`.

pub mod checksum;
pub mod codec;
mod error;
pub mod experiments;
mod record;
mod store;
pub mod testkit;

pub use error::{KvError, Result};
pub use record::{MAX_KEY_LEN, MAX_VALUE_LEN};
pub use store::Store;

/// `len` bytes that look random but are the same for the same `seed`, for tests.
#[cfg(test)]
fn test_bytes(seed: u64, len: usize) -> Vec<u8> {
    testkit::Rng::new(seed).bytes(len)
}
