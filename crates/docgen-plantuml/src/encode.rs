//! PlantUML text encoding for the server's `GET /svg/{encoded}` endpoint.
//!
//! The scheme is: UTF-8 → raw DEFLATE → PlantUML's custom base64 variant (which
//! uses the alphabet `0-9A-Za-z-_`, NOT standard base64). This is the canonical,
//! universally-supported way to pass a diagram to a PlantUML server in a URL.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

/// Encode PlantUML `source` into the URL path segment the server expects.
///
/// Panics never: a DEFLATE write to an in-memory buffer cannot fail.
pub fn encode(source: &str) -> String {
    let deflated = deflate(source.as_bytes());
    encode64(&deflated)
}

/// Raw DEFLATE (no zlib/gzip header) — what the PlantUML server decodes.
fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).expect("deflate to Vec cannot fail");
    enc.finish().expect("deflate finish to Vec cannot fail")
}

/// PlantUML's base64 variant over arbitrary bytes: each group of 3 bytes → 4
/// chars from the `0-9A-Za-z-_` alphabet. A trailing partial group is zero-padded
/// (the PlantUML reference `append3bytes`/`encode6bit`).
fn encode64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b1 = chunk[0];
        let b2 = chunk.get(1).copied().unwrap_or(0);
        let b3 = chunk.get(2).copied().unwrap_or(0);
        out.push(encode6bit(b1 >> 2));
        out.push(encode6bit(((b1 & 0x3) << 4) | (b2 >> 4)));
        out.push(encode6bit(((b2 & 0xF) << 2) | (b3 >> 6)));
        out.push(encode6bit(b3 & 0x3F));
    }
    out
}

/// Map a 6-bit value (0..64) to PlantUML's alphabet: `0-9`, then `A-Z`, then
/// `a-z`, then `-`, then `_`.
fn encode6bit(b: u8) -> char {
    let b = b & 0x3F;
    match b {
        0..=9 => (b'0' + b) as char,
        10..=35 => (b'A' + (b - 10)) as char,
        36..=61 => (b'a' + (b - 36)) as char,
        62 => '-',
        _ => '_', // 63
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode6bit_covers_the_full_alphabet() {
        assert_eq!(encode6bit(0), '0');
        assert_eq!(encode6bit(9), '9');
        assert_eq!(encode6bit(10), 'A');
        assert_eq!(encode6bit(35), 'Z');
        assert_eq!(encode6bit(36), 'a');
        assert_eq!(encode6bit(61), 'z');
        assert_eq!(encode6bit(62), '-');
        assert_eq!(encode6bit(63), '_');
    }

    #[test]
    fn encode64_matches_hand_computed_vectors() {
        // Three zero bytes → four '0's.
        assert_eq!(encode64(&[0, 0, 0]), "0000");
        // Three 0xFF bytes → all six-bit groups are 0x3F → four '_'.
        assert_eq!(encode64(&[0xFF, 0xFF, 0xFF]), "____");
        // A single byte 0x00 is zero-padded to a full group → "0000".
        assert_eq!(encode64(&[0x00]), "0000");
        // 0b0000_0100 = 4: c1 = 4>>2 = 1 -> '1'; c2 = (4&3)<<4 = 0 -> '0'; 0;0.
        assert_eq!(encode64(&[0x04]), "1000");
    }

    #[test]
    fn encode_is_deterministic_and_within_alphabet() {
        let src = "@startuml\nAlice -> Bob : hi\n@enduml\n";
        let a = encode(src);
        let b = encode(src);
        assert_eq!(a, b, "encoding must be deterministic");
        assert!(!a.is_empty());
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "encoded output must stay within the PlantUML URL alphabet: {a}"
        );
    }
}
