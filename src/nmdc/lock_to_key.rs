#[allow(dead_code)]
/// Convert an NMDC $Lock challenge into a $Key response.
///
/// The algorithm:
/// 1. key[0] = lock[0] ^ lock[n-1] ^ lock[n-2] ^ 5
/// 2. key[i] = lock[i] ^ lock[i-1]  for i in 1..n
/// 3. Nibble-swap every byte: (b << 4 | b >> 4) & 0xFF
/// 4. Escape bytes 0, 5, 36, 96, 124, 126 as /%DCNxxx%/
pub fn lock_to_key(lock: &[u8]) -> Vec<u8> {
    let len = lock.len();
    if len < 3 {
        return Vec::new();
    }

    let mut key = vec![0u8; len];

    // Step 1-2: XOR
    key[0] = lock[0] ^ lock[len - 1] ^ lock[len - 2] ^ 5;
    for i in 1..len {
        key[i] = lock[i] ^ lock[i - 1];
    }

    // Step 3: Nibble swap (using wrapping to avoid overflow panic in debug)
    for b in key.iter_mut() {
        *b = b.wrapping_shl(4) | b.wrapping_shr(4);
    }

    // Step 4: Encode special characters
    let mut result = Vec::with_capacity(len * 2);
    for &b in &key {
        match b {
            0 => result.extend_from_slice(b"/%DCN000%/"),
            5 => result.extend_from_slice(b"/%DCN005%/"),
            36 => result.extend_from_slice(b"/%DCN036%/"),
            96 => result.extend_from_slice(b"/%DCN096%/"),
            124 => result.extend_from_slice(b"/%DCN124%/"),
            126 => result.extend_from_slice(b"/%DCN126%/"),
            _ => result.push(b),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_lock() {
        // Test with a simple known lock string
        let lock = b"EXTENDEDPROTOCOL_hub_Pk=test";
        let key = lock_to_key(lock);
        assert!(!key.is_empty(), "Key should not be empty");
    }

    #[test]
    fn test_short_lock() {
        // Lock too short
        assert!(lock_to_key(b"ab").is_empty());
    }

    #[test]
    fn test_known_pair() {
        // Verify with a simple lock where we can trace the algorithm
        // Lock: "ABC" (len=3)
        // key[0] = A(65) ^ C(67) ^ B(66) ^ 5 = 65^67=2, 2^66=64, 64^5=69
        // key[1] = B(66) ^ A(65) = 3
        // key[2] = C(67) ^ B(66) = 1
        // Nibble swap: 69=0x45 -> 0x54=84, 3=0x03 -> 0x30=48, 1=0x01 -> 0x10=16
        let lock = b"ABC";
        let key = lock_to_key(lock);
        // 84='T', 48='0', 16 is not escaped
        assert_eq!(key[0], b'T');
        assert_eq!(key[1], b'0');
        assert_eq!(key[2], 16);
    }

    #[test]
    fn test_escaping() {
        // Verify that special bytes get escaped
        // Create a lock that produces byte 0 in the output
        // key[0] = lock[0] ^ lock[n-1] ^ lock[n-2] ^ 5
        // After nibble swap, if we get 0, it should be escaped

        // We can verify by checking the output contains /%DCN markers for special values
        let key = lock_to_key(b"ABCDEFGHIJ");
        let key_str = String::from_utf8_lossy(&key);
        // Just verify it doesn't panic and produces reasonable output
        assert!(!key.is_empty());
        // Check no raw null bytes in output
        assert!(!key.contains(&0u8) || key_str.contains("/%DCN000%/"));
    }

    #[test]
    fn test_roundtrip_consistency() {
        // Same input should always produce same output
        let lock = b"EXTENDEDPROTOCOL_hub";
        let key1 = lock_to_key(lock);
        let key2 = lock_to_key(lock);
        assert_eq!(key1, key2);
    }
}
