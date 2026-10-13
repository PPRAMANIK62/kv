//! `kvkit`, the storage library behind `kvctl`.

pub mod checksum;
pub mod codec;
mod store;

pub use store::Store;

/// `len` bytes that look random but are the same for the same `seed`, for tests.
#[cfg(test)]
fn test_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed;
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        // Knuth's MMIX linear congruential generator. It's high bits are
        // the most random ones, so each byte comes from the top of the state.
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        bytes.push((state >> 56) as u8);
    }
    bytes
}
