//! Turning numbers and byte strings into bytes and back.
//! Fixed width numbers are little-endian. Varints are unsigned LEB128,
//! the "base 128 varints" of Protocol Buffers.

/// Builds a byte string, one field at a time.
#[derive(Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Encoder {
        Encoder { buf: Vec::new() }
    }

    pub fn put_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Four bytes, least significant first.
    pub fn put_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// One to ten bytes, seven bits per byte, least significant group first.
    /// The top bit of each byte says whether another one follows.
    pub fn put_uvarint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.buf.push(value as u8 | 0x80);
            value >>= 7;
        }
        self.buf.push(value as u8);
    }

    /// The bytes as they are, wwith nothing to say where they end.
    pub fn put_bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The length of `bytes` as varint, then the bytes.
    pub fn put_len_prefixed(&mut self, bytes: &[u8]) {
        self.put_uvarint(bytes.len() as u64);
        self.put_bytes(bytes);
    }

    /// Everything written so far.
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Why a `Decoder` could not read a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The input ended before the value did.
    Truncated,
    /// A varint is longer than ten bytes or too large for a `u64`.
    Overflow,
}

/// Reads fields from the front of a byte slice. Every method either returns
/// a value or moves past it, or returns an error. After an error the decoder's
/// position is unspecified, so stop using it.
#[derive(Debug)]
pub struct Decoder<'a> {
    buf: &'a [u8],
}

impl<'a> Decoder<'a> {
    pub fn new(buf: &'a [u8]) -> Decoder<'a> {
        Decoder { buf }
    }

    /// How many bytes are left to read.
    pub fn remaining(&self) -> usize {
        self.buf.len()
    }

    pub fn get_u8(&mut self) -> Result<u8, DecodeError> {
        let Some((&byte, rest)) = self.buf.split_first() else {
            return Err(DecodeError::Truncated);
        };
        self.buf = rest;
        Ok(byte)
    }

    pub fn get_u32(&mut self) -> Result<u32, DecodeError> {
        let Some((&bytes, rest)) = self.buf.split_first_chunk::<4>() else {
            return Err(DecodeError::Truncated);
        };
        self.buf = rest;
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn get_uvarint(&mut self) -> Result<u64, DecodeError> {
        let mut value = 0;
        let mut shift = 0;
        loop {
            let byte = self.get_u8()?;
            // The tenth byte holds bit 63 only: 1 is the largest it can be.
            if shift == 63 && byte > 1 {
                return Err(DecodeError::Overflow);
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    /// The next `len` bytes.
    pub fn get_bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let Some((bytes, rest)) = self.buf.split_at_checked(len) else {
            return Err(DecodeError::Truncated);
        };
        self.buf = rest;
        Ok(bytes)
    }

    /// A varint length, then that many bytes.
    pub fn get_len_prefixed(&mut self) -> Result<&'a [u8], DecodeError> {
        let len = self.get_uvarint()?;
        if len > self.buf.len() as u64 {
            return Err(DecodeError::Truncated);
        }
        self.get_bytes(len as usize)
    }

    /// Everything that is left.
    pub fn rest(&mut self) -> &'a [u8] {
        let rest = self.buf;
        self.buf = &[];
        rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uvarint_bytes(value: u64) -> Vec<u8> {
        let mut encoder = Encoder::new();
        encoder.put_uvarint(value);
        encoder.finish()
    }

    #[test]
    fn fixed_width_numbers_are_little_endian() {
        let mut encoder = Encoder::new();
        encoder.put_u32(150);
        encoder.put_u8(7);
        let bytes = encoder.finish();
        assert_eq!(bytes, [0x96, 0x00, 0x00, 0x00, 0x07]);

        let mut decoder = Decoder::new(&bytes);
        assert_eq!(decoder.get_u32(), Ok(150));
        assert_eq!(decoder.get_u8(), Ok(7));
        assert_eq!(decoder.remaining(), 0);
    }

    #[test]
    fn varints_have_the_documented_bytes() {
        assert_eq!(uvarint_bytes(0), [0x00]);
        assert_eq!(uvarint_bytes(1), [0x01]);
        assert_eq!(uvarint_bytes(127), [0x7f]);
        assert_eq!(uvarint_bytes(128), [0x80, 0x01]);
        assert_eq!(uvarint_bytes(150), [0x96, 0x01]);
        assert_eq!(uvarint_bytes(300), [0xac, 0x02]);
        let mut max = vec![0xff; 9];
        max.push(0x01);
        assert_eq!(uvarint_bytes(u64::MAX), max);
    }

    #[test]
    fn varints_round_trip_at_every_size() {
        // 0, then 2^k - 1 and 2^k for every k: the edges of every length.
        let mut values = vec![0, u64::MAX];
        for k in 1..64 {
            values.push((1 << k) - 1);
            values.push(1 << k);
        }
        for value in values {
            let bytes = uvarint_bytes(value);
            let expected_len = (64 - value.leading_zeros()).div_ceil(7).max(1);
            assert_eq!(bytes.len(), expected_len as usize, "{value}");
            let mut decoder = Decoder::new(&bytes);
            assert_eq!(decoder.get_uvarint(), Ok(value));
            assert_eq!(decoder.remaining(), 0);
        }
    }

    #[test]
    fn a_cut_short_varint_is_truncated() {
        let bytes = uvarint_bytes(u64::MAX);
        for len in 0..bytes.len() {
            let mut decoder = Decoder::new(&bytes[..len]);
            assert_eq!(decoder.get_uvarint(), Err(DecodeError::Truncated));
        }
    }

    #[test]
    fn a_varint_too_big_for_u64_is_an_overflow() {
        // Ten bytes whose last one carries more than bit 63.
        let mut bytes = vec![0xff; 9];
        bytes.push(0x02);
        assert_eq!(
            Decoder::new(&bytes).get_uvarint(),
            Err(DecodeError::Overflow)
        );
        // Eleven bytes: the tenth still says "another byte follows".
        let bytes = [0x80; 11];
        assert_eq!(
            Decoder::new(&bytes).get_uvarint(),
            Err(DecodeError::Overflow)
        );
    }

    #[test]
    fn length_prefixed_bytes_round_trip() {
        let mut encoder = Encoder::new();
        encoder.put_len_prefixed(b"testing");
        encoder.put_len_prefixed(b"");
        encoder.put_bytes(b"rest");
        let bytes = encoder.finish();
        assert_eq!(&bytes[..8], b"\x07testing");

        let mut decoder = Decoder::new(&bytes);
        assert_eq!(decoder.get_len_prefixed(), Ok(&b"testing"[..]));
        assert_eq!(decoder.get_len_prefixed(), Ok(&b""[..]));
        assert_eq!(decoder.rest(), b"rest");
        assert_eq!(decoder.remaining(), 0);
    }

    #[test]
    fn a_length_longer_than_the_input_is_truncated() {
        // The length says 2^63 bytes follow. Three do.
        let mut encoder = Encoder::new();
        encoder.put_uvarint(1 << 63);
        encoder.put_bytes(b"abc");
        let bytes = encoder.finish();
        let mut decoder = Decoder::new(&bytes);
        assert_eq!(decoder.get_len_prefixed(), Err(DecodeError::Truncated));
    }
}
