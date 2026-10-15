//! CRC32C, the 32-bit cyclic redundancy check with Castagnoli's
//! Polynomial, as specified in RFC 3720 section 12.1.

/// The polynomial 0x1EDC6F41 with its bits reversed,
/// for a CRC that processes the low bit of each byte first.
const POLY: u32 = 0x82f6_3b78;

/// `TABLES[0][b]` is the remainder of dividing the byte `b` by the polynomial.
/// `TABLES[k][b]` is the same for `b` followed by `k` zero bytes.
/// The compiler computes them, so they cost nothing at run time.
static TABLES: [[u32; 256]; 8] = make_tables();

const fn make_tables() -> [[u32; 256]; 8] {
    let mut tables = [[0; 256]; 8];
    let mut b = 0;
    while b < 256 {
        // Divide one byte by the polynomial, one bit at a time.
        let mut crc = b as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        tables[0][b] = crc;
        b += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut b = 0;
        while b < 256 {
            // Push the previous table's remainder through one more zero byte.
            let prev = tables[k - 1][b];
            tables[k][b] = (prev >> 8) ^ tables[0][(prev & 0xff) as usize];
            b += 1;
        }
        k += 1;
    }
    tables
}

/// CRC32C of `data`, one byte and one table lookup at a time.
pub fn crc32c_bytewise(data: &[u8]) -> u32 {
    !update_bytewise(!0, data)
}

/// Feeds `data` into the CRC register `crc`, one byte at a time.
fn update_bytewise(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc = (crc >> 8) ^ TABLES[0][((crc ^ u32::from(byte)) & 0xff) as usize];
    }
    crc
}

/// CRC32C of `data`, eight bytes and eight table lookups at a time
/// ("slicing-by-8").
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc = !0;
    let (chunks, rest) = data.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_le_bytes(*chunk) ^ u64::from(crc);
        crc = TABLES[7][byte(word, 0)]
            ^ TABLES[6][byte(word, 1)]
            ^ TABLES[5][byte(word, 2)]
            ^ TABLES[4][byte(word, 3)]
            ^ TABLES[3][byte(word, 4)]
            ^ TABLES[2][byte(word, 5)]
            ^ TABLES[1][byte(word, 6)]
            ^ TABLES[0][byte(word, 7)];
    }
    !update_bytewise(crc, rest)
}

/// Byte `i` of `word`, counting from the least significant, as an index.
fn byte(word: u64, i: u32) -> usize {
    usize::from((word >> (8 * i)) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A function that computes a CRC32C.
    type CrcFn = fn(&[u8]) -> u32;

    /// Each version we have, by name.
    const VERSIONS: [(&str, CrcFn); 2] = [("bytewise", crc32c_bytewise), ("slice-by-8", crc32c)];

    #[test]
    fn rfc_3720_test_vectors() {
        let incrementing: Vec<u8> = (0..32).collect();
        let decrementing: Vec<u8> = (0..32).rev().collect();
        let read_command_pdu = [
            0x01, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x14,
            0x00, 0x00, 0x00, 0x18, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        // The digest bytes exactly as RFC 3720 appendix B.4 prints them.
        let vectors: [(&[u8], [u8; 4]); 5] = [
            (&[0x00; 32], [0xaa, 0x36, 0x91, 0x8a]),
            (&[0xff; 32], [0x43, 0xab, 0xa8, 0x62]),
            (&incrementing, [0x4e, 0x79, 0xdd, 0x46]),
            (&decrementing, [0x5c, 0xdb, 0x3f, 0x11]),
            (&read_command_pdu, [0x56, 0x3a, 0x96, 0xd9]),
        ];
        for (name, crc) in VERSIONS {
            for (data, digest) in vectors {
                assert_eq!(crc(data).to_le_bytes(), digest, "{name}");
            }
        }
    }

    #[test]
    fn check_values() {
        for (name, crc) in VERSIONS {
            assert_eq!(crc(b""), 0, "{name}");
            assert_eq!(crc(b"123456789"), 0xE306_9283, "{name}");
        }
    }

    #[test]
    fn slice_by_8_matches_bytewise_at_every_length_and_alignment() {
        let data = crate::test_bytes(1, 200);
        for start in 0..8 {
            for end in start..data.len() {
                let part = &data[start..end];
                assert_eq!(crc32c(part), crc32c_bytewise(part), "{start}..{end}");
            }
        }
    }

    #[test]
    fn both_versions_match_the_crc32c_crate() {
        for seed in 0..1000 {
            let data = crate::test_bytes(seed, seed as usize);
            let expected = ::crc32c::crc32c(&data);
            for (name, crc) in VERSIONS {
                assert_eq!(crc(&data), expected, "{name}, seed {seed}");
            }
        }
    }

    /// Prints how fast each version is. Run it with
    /// `cargo test --release -p kvkit checksum_speed -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn checksum_speed() {
        use std::hint::black_box;
        use std::time::Instant;

        let versions = [
            VERSIONS[0],
            VERSIONS[1],
            ("crc32c crate", ::crc32c::crc32c as CrcFn),
        ];
        // 30 bytes is what each record of `kvctl load` checksums.
        for size in [30, 4096, 1 << 20] {
            let mut data = crate::test_bytes(size as u64, size);
            let calls = (1 << 30) / size; // about 1 GiB in total
            for (name, crc) in versions {
                let mut ns_per_byte = Vec::new();
                for _ in 0..5 {
                    let mut result = 0;
                    let start = Instant::now();
                    for _ in 0..calls {
                        // Each input depends on the previous result, so the
                        // CPU can't work on two calls at once.
                        data[0] = result as u8;
                        result = crc(black_box(&data));
                    }
                    let elapsed = start.elapsed().as_nanos() as f64;
                    black_box(result);
                    ns_per_byte.push(elapsed / (calls * size) as f64);
                }
                ns_per_byte.sort_by(f64::total_cmp);
                let median = ns_per_byte[2];
                println!(
                    "{size:>8} bytes  {name:<13} {:>6.2} GB/s  {:>9.1} ns per call",
                    1.0 / median,
                    median * size as f64
                );
            }
        }
    }
}
