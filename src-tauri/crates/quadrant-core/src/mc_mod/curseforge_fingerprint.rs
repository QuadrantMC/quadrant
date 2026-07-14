use std::fs;

pub type Buffer = Vec<u8>;

pub fn get_jar_contents(path: &str) -> Buffer {
    fs::read(path).unwrap_or_default()
}

pub fn compute_hash(buffer: &Buffer) -> u32 {
    const MULTIPLEX: u32 = 1540483477;
    let normalized_length = compute_normalized_length(buffer);
    let mut num2 = 1 ^ normalized_length;
    let mut num3 = 0;
    let mut num4 = 0;

    for &b in buffer {
        if !is_whitespace_character(b) {
            num3 |= (b as u32) << num4;
            num4 += 8;
            if num4 == 32 {
                let num6 = num3.wrapping_mul(MULTIPLEX);
                let num7 = (num6 ^ (num6 >> 24)).wrapping_mul(MULTIPLEX);
                num2 = num2.wrapping_mul(MULTIPLEX) ^ num7;
                num3 = 0;
                num4 = 0;
            }
        }
    }

    if num4 > 0 {
        num2 = (num2 ^ num3).wrapping_mul(MULTIPLEX);
    }

    let num6 = (num2 ^ (num2 >> 13)).wrapping_mul(MULTIPLEX);
    num6 ^ (num6 >> 15)
}

pub fn compute_normalized_length(buffer: &Buffer) -> u32 {
    buffer
        .iter()
        .filter(|&&b| !is_whitespace_character(b))
        .count() as u32
}

pub fn is_whitespace_character(b: u8) -> bool {
    matches!(b, 9 | 10 | 13 | 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_whitespace_character_matches_curseforge_set() {
        for b in [9, 10, 13, 32] {
            assert!(is_whitespace_character(b));
        }
        for b in [0, 8, 11, 12, 14, 31, 33, b'a', 0xff] {
            assert!(!is_whitespace_character(b));
        }
    }

    #[test]
    fn compute_normalized_length_ignores_whitespace() {
        assert_eq!(compute_normalized_length(&Vec::new()), 0);
        assert_eq!(compute_normalized_length(&vec![9, 10, 13, 32]), 0);
        assert_eq!(
            compute_normalized_length(&b"\t\nhello\r world\t\r\n ".to_vec()),
            10
        );
        assert_eq!(
            compute_normalized_length(&vec![
                0x00, 0x01, 0xfe, 0xff, 0x7f, 0x80, 0x09, 0x0a, 0x0d, 0x20, 0x41
            ]),
            7
        );
    }

    #[test]
    fn compute_hash_golden_values() {
        assert_eq!(compute_hash(&Vec::new()), 1540447798);
        assert_eq!(compute_hash(&b"helloworld".to_vec()), 2824650221);
        assert_eq!(
            compute_hash(&vec![
                0x00, 0x01, 0xfe, 0xff, 0x7f, 0x80, 0x09, 0x0a, 0x0d, 0x20, 0x41
            ]),
            2087903364
        );
    }

    #[test]
    fn compute_hash_strips_whitespace_bytes() {
        assert_eq!(
            compute_hash(&b"\t\nhello\r world\t\r\n ".to_vec()),
            compute_hash(&b"helloworld".to_vec())
        );
        assert_eq!(
            compute_hash(&vec![9, 10, 13, 32, 32, 13, 10, 9]),
            compute_hash(&Vec::new())
        );
    }

    #[test]
    fn get_jar_contents_returns_empty_for_missing_file() {
        assert!(get_jar_contents("/nonexistent/quadrant-test.jar").is_empty());
    }
}
