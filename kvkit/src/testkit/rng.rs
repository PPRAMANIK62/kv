//! A small, seeded pseudorandom number generator. The same seed gives the
//! same numbers on every machine and in every build.

/// 2^64 divided by the golden ratio, an odd number with no pattern in it's bits.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// Advances `state` by one step of Vigna's splitmix64 and returns the
/// output, exactly as `splitmix64.c` at <https://prng.di.unimi.it/> does.
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(GAMMA);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A xorshift64* generator, seeded through splitmix64.
#[derive(Debug, Clone)]
pub struct Rng {
    /// Never zero: zero is the one state xorshift can't leave.
    state: u64,
}

impl Rng {
    /// A generator whose numbers depend only on `seed`. Any seed is fine,
    /// including 0, and seeds that differ by one bit give unrelated streams.
    pub fn new(seed: u64) -> Rng {
        let mut mixer = seed;
        let state = splitmix64(&mut mixer);
        // splitmix64 returns 0 for exactly one seed. Don't let it be ours.
        Rng {
            state: if state == 0 { GAMMA } else { state },
        }
    }

    /// The next 64 random bits: Marsaglia's xorshift with Vigna's shifts
    /// (12, 25, 27), then multiplication that mixes every bit of the
    /// state into the high bits of the result.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number in `0..n`. `n` must not be 0.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0) has no possible result");
        // Read the 64 bits as a fraction of 2^64, in 0..1, and scale it to
        // 0..n. This keeps the high bits, the best ones, and is off from
        // perfectly even by at most n / 2^64.
        ((u128::from(self.next_u64()) * u128::from(n)) >> 64) as u64
    }

    /// A number in `low..=high`.
    pub fn between(&mut self, low: usize, high: usize) -> usize {
        low + self.below((high - low + 1) as u64) as usize
    }

    /// `len` random bytes.
    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push((self.next_u64() >> 56) as u8);
        }
        bytes
    }

    /// One element of `items`, which must not be empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_matches_the_reference_code() {
        // Printed by Vigna's splitmix64.c, compiled with gcc, for each
        // starting state.
        let expected: [(u64, [u64; 3]); 2] = [
            (
                0,
                [0xe220a8397b1dcdaf, 0x6e789e6aa1b965f4, 0x06c45d188009454f],
            ),
            (
                1234567,
                [0x599ed017fb08fc85, 0x2c73f08458540fa5, 0x883ebce5a3f27c77],
            ),
        ];
        for (start, outputs) in expected {
            let mut state = start;
            for output in outputs {
                assert_eq!(splitmix64(&mut state), output, "start {start}");
            }
        }
    }

    #[test]
    fn xorshift64_star_matches_the_reference_code() {
        // Printed by the C code in figure 10 of Vigna's xorshift paper,
        // starting from x = 1.
        let mut rng = Rng { state: 1 };
        assert_eq!(rng.next_u64(), 0x47e4ce4b896cdd1d);
        assert_eq!(rng.next_u64(), 0xabcfa6a8e079651d);
        assert_eq!(rng.next_u64(), 0xb9d10d8feb731f57);
    }

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn nearby_seeds_give_unrelated_numbers() {
        // Seeds 0 to 3, and seed 0 isn't stuck. For an unseeded xorshift,
        // the stream of seed 3 is the XOR of the streams of seeds 1 and 2.
        let mut rngs: Vec<Rng> = (0..4).map(Rng::new).collect();
        for _ in 0..100 {
            let x: Vec<u64> = rngs.iter_mut().map(|rng| rng.next_u64()).collect();
            assert_ne!(x[0], 0);
            assert_ne!(x[3], x[1] ^ x[2]);
        }
    }

    #[test]
    fn below_stays_in_range_and_reaches_every_value() {
        let mut rng = Rng::new(7);
        let mut seen = [0; 6];
        for _ in 0..6000 {
            seen[rng.below(6) as usize] += 1;
        }
        // Each value should come up about 1,000 times.
        for count in seen {
            assert!((850..1150).contains(&count), "{seen:?}");
        }
    }

    #[test]
    fn low_bits_do_not_repeat_a_short_cycle() {
        // An LCG's lowest two bits repeat every 4 outputs. Ours mustn't.
        let mut rng = Rng::new(1);
        let low: Vec<u64> = (0..64).map(|_| rng.next_u64() % 4).collect();
        assert!((0..60).any(|i| low[i] != low[i + 4]), "{low:?}");
    }
}
