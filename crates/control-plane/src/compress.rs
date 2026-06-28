//! zstd level-3 compress/decompress helpers.
//!
//! For MVP: plain zstd level 3 with no trained dictionary.
//! Dictionary training is a post-MVP optimization (ADR §10A.2).
//! TODO: trained dictionary, ADR §10A.2.

use anyhow::Result;

/// zstd compression level used for MVP message bodies.
const LEVEL: i32 = 3;

/// Compress a byte slice with zstd at level 3.
pub fn compress(data: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::encode_all(data, LEVEL)?)
}

/// Decompress a zstd-compressed byte slice.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::decode_all(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_arbitrary_bytes() {
        let original = b"the quick brown fox jumps over the lazy dog";
        let compressed = compress(original).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn compressed_is_smaller_for_repetitive_string() {
        // Highly repetitive input should compress well: compressed must be
        // smaller than the original.
        let original = "Hello ".repeat(1000);
        let compressed = compress(original.as_bytes()).unwrap();
        assert!(
            compressed.len() < original.len(),
            "compressed {} should be < original {}",
            compressed.len(),
            original.len()
        );
    }

    #[test]
    fn compressed_is_roughly_half_or_better_for_repetitive() {
        // The plan's verify test asserts < original/2 for "Hello ".repeat(1000).
        let original = "Hello ".repeat(1000);
        let compressed = compress(original.as_bytes()).unwrap();
        assert!(
            compressed.len() < original.len() / 2,
            "compressed {} should be < original/2 {}",
            compressed.len(),
            original.len() / 2
        );
    }

    #[test]
    fn empty_input_roundtrips() {
        let compressed = compress(b"").unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, b"");
    }
}
