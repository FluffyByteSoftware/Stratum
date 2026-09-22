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

use crate::CharacterSummary;

/// The biggest length a frame is allowed to claim.  Plenty for a login or a
/// character list, and it stops somebody claiming a 4 GB packet and making
/// us wait for it.
pub const MAX_PACKET_BYTES: usize = 4096;

/// What a client has to say before it gets to try a password.  Not real
/// security -- anybody with a copy of the client has it.  It turns away
/// port scanners and bots that don't know what they are talking to.  It may
/// end up changing with the date.
pub const SECRET_WORD: &str = "potato";

/// The first byte of an AuthenticationResult.
pub const RESULT_SUCCESS: u8 = 0;
pub const RESULT_AUTHENTICATION_FAILED: u8 = 1;
/// The password was right, but the account is already logged in somewhere
/// else.  The client asks the player what to do and answers with a
/// SessionChoice.  Only ever sent after the right password, so it tells a
/// stranger nothing.
pub const RESULT_ALREADY_LOGGED_IN: u8 = 2;

/// The string that rides along with each result, for the player to see.
/// One failure message for everything, so a wrong secret word, a wrong name
/// and a wrong password all look the same from outside.
pub const SUCCESS_MESSAGE: &str = "Welcome to Stratum.";
pub const FAILURE_MESSAGE: &str = "Invalid Credentials";
pub const ALREADY_LOGGED_IN_MESSAGE: &str = "This account is already logged in.";

/// The first byte of a CharacterResult.  Unlike the login, a refusal here
/// comes with the real reason, because the player is already in.
pub const CHARACTER_DONE: u8 = 0;
pub const CHARACTER_REFUSED: u8 = 1;

/// Every packet type there is so far.  The high four bits say the group and
/// the low four say which one inside it.  0x1_ is Login, 0x2_ is
/// CharacterSelect, and Game (0x3_) comes later.  0xF_ is for testing.
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
    /// Client to server, after an AuthenticationResult of 2.  One byte: 0
    /// logs the other session out, 1 hangs this one up and leaves the other
    /// alone.  (So a shared account doesn't kick your brother off because
    /// you wanted to play.)
    SessionChoice = 0x15,
    /// Server to client, on the connection that just got logged out from
    /// somewhere else.  No payload, and the server closes it straight after.
    LoggedOutElsewhere = 0x16,
    /// Server to client.  A byte for how many slots the account has, a byte
    /// for how many characters are in them, then each character: its short
    /// name, its long name, a byte saying whether it can be played, and
    /// where it is.
    CharacterList = 0x20,
    /// Client to server.  One string: the new character's name.
    CreateCharacter = 0x21,
    /// Client to server.  One string: the name of the character to delete.
    DeleteCharacter = 0x22,
    /// Client to server.  One string: the name of the character to play.
    EnterWorld = 0x23,
    /// Server to client, after a create, a delete, or an EnterWorld that
    /// didn't work.  A result byte, then a string for the player.
    CharacterResult = 0x24,
    /// Server to client, after an EnterWorld that did.  The login token (a
    /// string), then the UDP port to take it to (a u16).
    WorldTicket = 0x25,
    /// Client to server. No payload. "Send me my character list again."
    /// The server answers with a CharacterList.
    RequestCharacterList = 0x26,
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
            0x15 => Some(PacketType::SessionChoice),
            0x16 => Some(PacketType::LoggedOutElsewhere),
            0x20 => Some(PacketType::CharacterList),
            0x21 => Some(PacketType::CreateCharacter),
            0x22 => Some(PacketType::DeleteCharacter),
            0x23 => Some(PacketType::EnterWorld),
            0x24 => Some(PacketType::CharacterResult),
            0x25 => Some(PacketType::WorldTicket),
            0x26 => Some(PacketType::RequestCharacterList),
            0xF0 => Some(PacketType::SimpleTcpMesg),
            _ => None,
        }
    }
}

/// What the player picked when their account was already logged in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    /// Log the other session out and carry on with this one.
    LogTheOtherOut,
    /// Leave the other session alone and hang this one up.
    Disconnect,
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

/// An AuthenticationResult of 2: the password was right, and the account
/// is already on somewhere else.
pub fn already_logged_in() -> Vec<u8> {
    let mut payload = vec![RESULT_ALREADY_LOGGED_IN];
    put_string(&mut payload, ALREADY_LOGGED_IN_MESSAGE);
    frame(PacketType::AuthenticationResult, &payload)
}

pub fn logged_out_elsewhere() -> Vec<u8> {
    frame(PacketType::LoggedOutElsewhere, &[])
}

/// The characters on an account, in the order they were made.  Each one is
/// its short name (what the client sends back to pick it), its long name
/// (what the player sees), 1 if it can be played or 0 if its file is
/// missing or damaged, and its position as three f32s.
///
/// The count is one byte, so a list longer than 255 would be cut short.
/// With 3 slots it never gets near that.
pub fn character_list(slots: u8, characters: &[CharacterSummary]) -> Vec<u8> {
    let count = characters.len().min(u8::MAX as usize);
    let mut payload = vec![slots, count as u8];
    for character in characters.iter().take(count) {
        put_string(&mut payload, &character.shortname);
        put_string(&mut payload, &character.longname);
        payload.push(if character.playable { 1 } else { 0 });
        payload.extend_from_slice(&character.x.to_le_bytes());
        payload.extend_from_slice(&character.y.to_le_bytes());
        payload.extend_from_slice(&character.z.to_le_bytes());
    }
    frame(PacketType::CharacterList, &payload)
}

pub fn character_result(success: bool, message: &str) -> Vec<u8> {
    let result = if success { CHARACTER_DONE } else { CHARACTER_REFUSED };
    let mut payload = vec![result];
    put_string(&mut payload, message);
    frame(PacketType::CharacterResult, &payload)
}

/// The login token and where to take it.  The port rides along, so the
/// client never has to be told it separately.
pub fn world_ticket(token: &str, udp_port: u16) -> Vec<u8> {
    let mut payload = Vec::new();
    put_string(&mut payload, token);
    payload.extend_from_slice(&udp_port.to_le_bytes());
    frame(PacketType::WorldTicket, &payload)
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
/// SecretWord, a CreateCharacter, a DeleteCharacter, an EnterWorld or a
/// SimpleTcpMesg.
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

/// The payload of a SessionChoice: exactly one byte, 0 or 1.
pub fn read_session_choice(payload: &[u8]) -> Result<Choice, String> {
    match payload {
        [0] => Ok(Choice::LogTheOtherOut),
        [1] => Ok(Choice::Disconnect),
        [other] => Err(format!("a session choice of {}, and only 0 and 1 mean anything", other)),
        _ => Err(format!("a session choice of {} bytes, and it should be 1", payload.len())),
    }
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
    fn already_logged_in_is_result_two() {
        let mut buffer = already_logged_in();
        let packet = take_packet(&mut buffer).unwrap().unwrap();

        assert_eq!(packet.kind, PacketType::AuthenticationResult as u8);
        assert_eq!(packet.payload[0], RESULT_ALREADY_LOGGED_IN);
        let mut at = 1;
        assert_eq!(take_string(&packet.payload, &mut at), Ok(ALREADY_LOGGED_IN_MESSAGE.to_string()));
        assert_eq!(finished(&packet.payload, at), Ok(()));
    }

    #[test]
    fn a_character_list_in_bytes() {
        // 3 slots, 1 character.  The length is 28: the type, the two
        // bytes, two strings of 4 + 2, the playable byte, and three f32s.
        let characters = vec![CharacterSummary {
            shortname: "ab".to_string(),
            longname: "Ab".to_string(),
            playable: true,
            x: 1.0,
            y: 0.0,
            z: -2.0,
        }];
        assert_eq!(character_list(3, &characters),
                   vec![28, 0, 0, 0, 0x20, 3, 1,
                        2, 0, 0, 0, b'a', b'b',
                        2, 0, 0, 0, b'A', b'b',
                        1,
                        0x00, 0x00, 0x80, 0x3F,
                        0x00, 0x00, 0x00, 0x00,
                        0x00, 0x00, 0x00, 0xC0]);

        // No characters yet is still a list: the slots, and a count of 0.
        assert_eq!(character_list(3, &[]), vec![3, 0, 0, 0, 0x20, 3, 0]);
    }

    #[test]
    fn a_world_ticket_in_bytes() {
        // 9998 is 0x270E, so the port goes out as 0E 27.
        assert_eq!(world_ticket("ab", 9998),
                   vec![9, 0, 0, 0, 0x25, 2, 0, 0, 0, b'a', b'b', 0x0E, 0x27]);
    }

    #[test]
    fn session_choices() {
        assert_eq!(read_session_choice(&[0]), Ok(Choice::LogTheOtherOut));
        assert_eq!(read_session_choice(&[1]), Ok(Choice::Disconnect));
        assert!(read_session_choice(&[2]).is_err());
        assert!(read_session_choice(&[]).is_err());
        assert!(read_session_choice(&[0, 0]).is_err());
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
        let every = [
            PacketType::Hello,
            PacketType::SecretWord,
            PacketType::AwaitingAuthentication,
            PacketType::AuthenticationRequest,
            PacketType::AuthenticationResult,
            PacketType::SessionChoice,
            PacketType::LoggedOutElsewhere,
            PacketType::CharacterList,
            PacketType::CreateCharacter,
            PacketType::DeleteCharacter,
            PacketType::EnterWorld,
            PacketType::CharacterResult,
            PacketType::WorldTicket,
            PacketType::RequestCharacterList,
            PacketType::SimpleTcpMesg];
        for kind in every {
            assert_eq!(PacketType::from_byte(kind as u8), Some(kind));
        }
        assert_eq!(PacketType::from_byte(0x00), None);
        assert_eq!(PacketType::from_byte(0x17), None);
        assert_eq!(PacketType::from_byte(0x27), None);
    }
}