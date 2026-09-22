//! File:     stratum-tools/src/fingerprinter.rs
//! Project:  Stratum Core
//! Author:   Jacob Chacko
//!
//! Fingerprinter, the UUID maker.  Anything on the server that needs a name
//! nothing else will ever have (an account, a character, later a login
//! token) gets one from here.  It started life as a private function in
//! account.rs, and moved out when the game needed it too.
//!
//! A UUID here is 16 bytes from the kernel's random source, with two of
//! them bent to mark it as a "version 4" (random) UUID, written in the
//! usual dashed form:
//!
//! ```text
//! 3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e10
//! ```
//!
//! No crate.  The standard library can open /dev/urandom, and that is all
//! this needs.  Linux only, for now, the same as the rest of the server.

use std::fs::File;
use std::io::{self, Read};

/// A new random UUID, in the usual form.  The only way it fails is if
/// /dev/urandom can't be read, which means something is badly wrong with
/// the machine.
// TODO(tokens): the login tokens will need random bytes too.  When they
// arrive, the /dev/urandom read becomes a function of its own in here, and
// both use it.
pub fn new_uuid() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;

    // The top four bits of byte 6 say the version (4).  The top two bits of
    // byte 8 say which UUID layout this is (the standard one).
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let mut text = String::new();
    for (position, byte) in bytes.iter().enumerate() {
        if position == 4 || position == 6 || position == 8 || position == 10 {
            text.push('-');
        }
        text.push_str(&format!("{:02x}", byte));
    }
    Ok(text)
}

/// True for text in the shape of one of our UUIDs: 36 characters, lowercase
/// hex, dashes in the right places, version 4.  It says nothing about
/// whether anything has that UUID.  For checking a file that claims to
/// hold one.
pub fn looks_like_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (position, byte) in bytes.iter().enumerate() {
        let wanted_dash = position == 8 || position == 13 || position == 18 || position == 23;
        let is_dash = *byte == b'-';
        if wanted_dash != is_dash {
            return false;
        }
        if !is_dash && !(byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)) {
            return false;
        }
    }
    bytes[14] == b'4' && b"89ab".contains(&bytes[19])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_look_right() {
        let first = new_uuid().unwrap();
        let second = new_uuid().unwrap();

        assert_eq!(first.len(), 36);
        for position in [8, 13, 18, 23] {
            assert_eq!(first.as_bytes()[position], b'-');
        }
        assert_eq!(first.as_bytes()[14], b'4');
        assert!("89ab".contains(first.as_bytes()[19] as char));
        assert!(first.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert!(!first.chars().any(|c| c.is_ascii_uppercase()));

        assert_ne!(first, second);
    }

    #[test]
    fn our_own_uuids_pass_the_shape_check() {
        for _ in 0..20 {
            assert!(looks_like_uuid(&new_uuid().unwrap()));
        }
        assert!(looks_like_uuid("3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e10"));
    }

    #[test]
    fn things_that_are_not_our_uuids_fail_it() {
        assert!(!looks_like_uuid(""));
        assert!(!looks_like_uuid("3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e1"));
        assert!(!looks_like_uuid("3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8e100"));
        // Uppercase, which we never write.
        assert!(!looks_like_uuid("3F2A91C0-E4B7-4D1A-9C0E-2B7F5A6D8E10"));
        // A dash in the wrong place.
        assert!(!looks_like_uuid("3f2a91c0e-4b7-4d1a-9c0e-2b7f5a6d8e10"));
        // Not version 4.
        assert!(!looks_like_uuid("3f2a91c0-e4b7-1d1a-9c0e-2b7f5a6d8e10"));
        // The wrong layout bits.
        assert!(!looks_like_uuid("3f2a91c0-e4b7-4d1a-1c0e-2b7f5a6d8e10"));
        // Not hex.
        assert!(!looks_like_uuid("3f2a91c0-e4b7-4d1a-9c0e-2b7f5a6d8g10"));
    }
}