//! File:     stratum-networking/src/protocol.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The packets the server and its clients agree on, down to the byte.  This
//! is the Rust half.  The C# half lives with Probe, and docs/PROTOCOL.md is
//! what both halves are written from.  When a half disagrees with the
//! document, the half is what gets fixed.
//!
//! Every packet goes out in the same frame:
//!
//! ```text
//! [length: u32][type: u8][payload]
//! ```
//!
//! The length counts the type byte and the payload, not itself.  TCP is one
//! long stream of bytes with no gaps in it, so without the length the
//! reader couldn't tell where one packet stops and the next one starts.
//!
//! Numbers are little-endian (lowest byte first), because that is what C#'s
//! BinaryWriter and BinaryReader do, and both clients are C#.  A string is a
//! u32 byte count and then that many bytes of UTF-8.
//!
//! Nothing in here logs or touches the network.  It turns packets into
//! bytes and bytes back into packets, and that is what lets the tests run.

/// The biggest length a frame is allowed to claim.  Plenty for a login,
/// and it stops somebody claiming a 4 GB packet and making us wait for it.
pub const MAX_PACKET_BYTES: usize = 4096;

/// What a client has to say before it gets to try a password.  Not real
/// security -- anybody with a copy of the client has it.  It turns away
/// port scanners and bots that don't know what they are talking to.  It may
/// end up changing with the date.
pub const SECRET_WORD: &str = "potato";

/// The first byte of an AuthenticationResult.
pub const RESULT_SUCCESS: u8 = 0;
pub const RESULT_AUTHENTICATION_FAILED: u8 = 1;

/// The string that rides along with each result, for the player to see.
/// One failure message for everything, so a wrong secret word, a wrong name
/// and a wrong password all look the same from outside.
pub const SUCCESS_MESSAGE: &str = "Welcome to Stratum.";
pub const FAILURE_MESSAGE: &str = "Invalid Credentials";

/// Every packet type there is so far.  The high four bits say the group and
/// the low four say which one inside it.  0x1_ is Login.  CharacterSelect
/// (0x2_) and Game (0x3_) come later.  0xF_ is for testing.
// Rust note: `repr(u8)` stores the enum as one byte, and `as u8` turns a
// value back into its number, the same as a C# `enum : byte`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PacketType {
    /// Server to client, right after TLS.  "Say the secret word."  No
    /// payload.
    Hello = 0x10,
    /// Client to server.  One string: the secret word.
    SecretWord = 0x11,
    /// Server to client.  "Now the username and password."  No payload.
    AwaitingAuthentication = 0x12,
    /// Client to server.  Two strings: the username, then the password.
    AuthenticationRequest = 0x13,
    /// Server to client.  A result byte, then a string for the player.
    AuthenticationResult = 0x14,
    /// Either way, once logged in.  One string.  The server sends it
    /// straight back, which is how we test that both directions work.
    SimpleTcpMesg = 0xF0,
}

impl PacketType {
    /// The packet type for a byte off the wire, or `None` for a byte that
    /// isn't one.
    pub fn from_byte(byte: u8) -> Option<PacketType> {
        match byte {
            0x10 => Some(PacketType::Hello),
            0x11 => Some(PacketType::SecretWord),
            0x12 => Some(PacketType::AwaitingAuthentication),
            0x13 => Some(PacketType::AuthenticationRequest),
            0x14 => Some(PacketType::AuthenticationResult),
            0xF0 => Some(PacketType::SimpleTcpMesg),
            _ => None,
        }
    }
}

/// One whole packet, taken off the wire.  The type is kept as the raw byte,
/// so a type we don't know can still be logged.
#[derive(Debug, PartialEq)]
pub struct Packet {
    pub kind: u8,
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// A packet as the bytes that go on the wire.
pub fn frame(kind: PacketType, payload: &[u8]) -> Vec<u8> {
    let length = (1 + payload.len()) as u32;
    let mut bytes = Vec::with_capacity(4 + length as usize);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.push(kind as u8);
    bytes.extend_from_slice(payload);
    bytes
}

/// Takes one whole packet off the front of `buffer`, if there is one.
/// `Ok(None)` means there isn't a whole one yet, and `buffer` is left alone
/// until more bytes arrive.  An `Err` means the frame is one we won't take,
/// and the connection should close.
///
/// The length gets checked as soon as its four bytes are in, so somebody
/// claiming a huge packet is turned away before we wait for any of it.
pub fn take_packet(buffer: &mut Vec<u8>) -> Result<Option<Packet>, String> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let length = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    if length == 0 {
        return Err("a packet with no type byte".to_string());
    }
    if length > MAX_PACKET_BYTES {
        return Err(format!("a packet of {} bytes, and the most we take is {}",
                           length, MAX_PACKET_BYTES));
    }
    if buffer.len() < 4 + length {
        return Ok(None);
    }

    let kind = buffer[4];
    let payload = buffer[5..4 + length].to_vec();
    // Rust note: `drain` takes those bytes out of the front of the Vec and
    // slides whatever is left down to the start.
    buffer.drain(..4 + length);
    Ok(Some(Packet { kind, payload }))
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

fn put_string(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

/// Reads the string that starts at `*at` in `payload`, and moves `*at` past
/// it, so the next read picks up where this one stopped.
fn take_string(payload: &[u8], at: &mut usize) -> Result<String, String> {
    let rest = &payload[*at..];
    if rest.len() < 4 {
        return Err("a string cut off before its length".to_string());
    }
    let length = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
    if rest.len() - 4 < length {
        return Err("a string cut off partway".to_string());
    }
    let text = String::from_utf8(rest[4..4 + length].to_vec())
        .map_err(|_| "a string that isn't UTF-8".to_string())?;
    *at += 4 + length;
    Ok(text)
}

/// Says no if anything is left over after the last field.  A packet with
/// extra bytes on the end was built by something that doesn't agree with
/// us about the protocol, and we would rather hear about it than guess.
fn finished(payload: &[u8], at: usize) -> Result<(), String> {
    if at == payload.len() {
        Ok(())
    } else {
        Err(format!("{} byte(s) left over at the end", payload.len() - at))
    }
}

// ---------------------------------------------------------------------------
// The packets the server sends
// ---------------------------------------------------------------------------

pub fn hello() -> Vec<u8> {
    frame(PacketType::Hello, &[])
}

pub fn awaiting_authentication() -> Vec<u8> {
    frame(PacketType::AwaitingAuthentication, &[])
}

pub fn authentication_result(success: bool, message: &str) -> Vec<u8> {
    let result = if success { RESULT_SUCCESS } else { RESULT_AUTHENTICATION_FAILED };
    let mut payload = vec![result];
    put_string(&mut payload, message);
    frame(PacketType::AuthenticationResult, &payload)
}

pub fn simple_message(text: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    put_string(&mut payload, text);
    frame(PacketType::SimpleTcpMesg, &payload)
}

// ---------------------------------------------------------------------------
// The packets the server reads
// ---------------------------------------------------------------------------

/// The payload of a packet that holds one string and nothing else: a
/// SecretWord or a SimpleTcpMesg.
pub fn read_one_string(payload: &[u8]) -> Result<String, String> {
    let mut at = 0;
    let text = take_string(payload, &mut at)?;
    finished(payload, at)?;
    Ok(text)
}

/// The payload of an AuthenticationRequest: the username, then the
/// password.
pub fn read_authentication_request(payload: &[u8]) -> Result<(String, String), String> {
    let mut at = 0;
    let username = take_string(payload, &mut at)?;
    let password = take_string(payload, &mut at)?;
    finished(payload, at)?;
    Ok((username, password))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// What a client would send, built the long way.
    fn request(username: &str, password: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        put_string(&mut payload, username);
        put_string(&mut payload, password);
        payload
    }

    #[test]
    fn a_hello_is_five_bytes() {
        assert_eq!(hello(), vec![1, 0, 0, 0, 0x10]);
    }

    #[test]
    fn numbers_go_lowest_byte_first() {
        // Spelled out byte by byte, because this is exactly what the C#
        // side has to match.  The length is 8: the type, the result, and a
        // string of 2.
        assert_eq!(authentication_result(false, "ab"),
                   vec![8, 0, 0, 0, 0x14, 1, 2, 0, 0, 0, b'a', b'b']);
    }

    #[test]
    fn a_packet_comes_back_out_of_its_frame() {
        let mut buffer = simple_message("hello");
        let packet = take_packet(&mut buffer).unwrap().unwrap();

        assert_eq!(packet.kind, PacketType::SimpleTcpMesg as u8);
        assert_eq!(read_one_string(&packet.payload), Ok("hello".to_string()));
        assert!(buffer.is_empty());
    }

    #[test]
    fn half_a_packet_waits_for_the_rest() {
        let whole = simple_message("hello");
        for cut in 0..whole.len() {
            let mut buffer = whole[..cut].to_vec();
            assert_eq!(take_packet(&mut buffer), Ok(None));
            assert_eq!(buffer.len(), cut);
        }
    }

    #[test]
    fn two_packets_in_one_read_come_out_one_at_a_time() {
        let mut buffer = hello();
        buffer.extend(awaiting_authentication());

        assert_eq!(take_packet(&mut buffer).unwrap().unwrap().kind, 0x10);
        assert_eq!(take_packet(&mut buffer).unwrap().unwrap().kind, 0x12);
        assert_eq!(take_packet(&mut buffer), Ok(None));
    }

    #[test]
    fn bad_lengths_are_refused() {
        let mut empty = vec![0, 0, 0, 0];
        assert!(take_packet(&mut empty).is_err());

        // 4097, one more than we take.  Refused on the length alone.
        let mut huge = vec![0x01, 0x10, 0, 0];
        assert!(take_packet(&mut huge).is_err());

        // Exactly the most we take is fine, once it has all arrived.
        let mut biggest = vec![0x00, 0x10, 0, 0];
        biggest.extend(vec![0xF0; MAX_PACKET_BYTES]);
        assert!(take_packet(&mut biggest).unwrap().is_some());
    }

    #[test]
    fn a_login_request_reads_back() {
        let payload = request("jacob", "Correct horse 1!");
        assert_eq!(read_authentication_request(&payload),
                   Ok(("jacob".to_string(), "Correct horse 1!".to_string())));
    }

    #[test]
    fn broken_payloads_are_refused() {
        let payload = request("jacob", "Correct horse 1!");

        // Cut off anywhere short of the end.
        for cut in 0..payload.len() {
            assert!(read_authentication_request(&payload[..cut]).is_err());
        }

        // Something extra on the end.
        let mut longer = payload.clone();
        longer.push(0);
        assert!(read_authentication_request(&longer).is_err());

        // A string that isn't UTF-8.
        let mut not_text = Vec::new();
        not_text.extend_from_slice(&2u32.to_le_bytes());
        not_text.extend_from_slice(&[0xFF, 0xFE]);
        assert!(read_one_string(&not_text).is_err());

        // A string that claims to be longer than the packet.
        let mut liar = Vec::new();
        liar.extend_from_slice(&1000u32.to_le_bytes());
        liar.extend_from_slice(b"short");
        assert!(read_one_string(&liar).is_err());
    }

    #[test]
    fn every_type_survives_its_byte() {
        let every = [PacketType::Hello, PacketType::SecretWord, PacketType::AwaitingAuthentication,
            PacketType::AuthenticationRequest, PacketType::AuthenticationResult,
            PacketType::SimpleTcpMesg];
        for kind in every {
            assert_eq!(PacketType::from_byte(kind as u8), Some(kind));
        }
        assert_eq!(PacketType::from_byte(0x00), None);
        assert_eq!(PacketType::from_byte(0x15), None);
    }
}