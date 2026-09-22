# Stratum -- The Protocol

What the server and a client say to each other, down to the byte.  The server's half is `stratum-networking/src/protocol.rs`.  The C# half lives with the client.  This document is what both are written from, so when a half disagrees with it, the half is wrong.

Only the TCP side exists so far, with two groups: Login and CharacterSelect.

## The connection

TCP, and TLS 1.3 from the first byte.  The server's certificate is self-signed, so a client doesn't ask anybody to vouch for it.  It keeps its own copy of `cert.pem` and refuses any server that shows it a different one.  Everything below travels inside the TLS.

## The frame

Every packet, both ways, looks like this:

```text
[length: u32][type: u8][payload]
```

The length counts the type byte and the payload, not itself.  So a packet with no payload has a length of 1.  TCP is one long stream of bytes, and the length is how the reader knows where one packet ends and the next starts.  A length of 0, or more than 4096, closes the connection.

All numbers are **little-endian** (lowest byte first).  That is what C#'s `BinaryWriter` and `BinaryReader` do already.

A **string** is a `u32` byte count, then that many bytes of UTF-8.  No terminating zero.

A payload with bytes left over after its last field is refused, the same as one that is cut short.

## Packet types

The high four bits are the group and the low four are the packet inside it.

| Group | Types |
|---|---|
| Login | 0x10 - 0x1F |
| CharacterSelect | 0x20 - 0x2F |
| Game | 0x30 - 0x3F (later) |
| Testing | 0xF0 - 0xFF |

| Type | Name | Direction | Payload |
|---|---|---|---|
| 0x10 | Hello | server to client | none |
| 0x11 | SecretWord | client to server | string |
| 0x12 | AwaitingAuthentication | server to client | none |
| 0x13 | AuthenticationRequest | client to server | string username, string password |
| 0x14 | AuthenticationResult | server to client | `u8` result, string message |
| 0x15 | SessionChoice | client to server | `u8` choice |
| 0x16 | LoggedOutElsewhere | server to client | none |
| 0x20 | CharacterList | server to client | `u8` slots, `u8` count, then that many strings |
| 0x21 | CreateCharacter | client to server | string name |
| 0x22 | DeleteCharacter | client to server | string name |
| 0x23 | EnterWorld | client to server | string name |
| 0x24 | CharacterResult | server to client | `u8` result, string message |
| 0x25 | WorldTicket | server to client | string token, `u16` UDP port |
| 0xF0 | SimpleTcpMesg | either way | string |

Every packet has its own section further down, with example bytes.

## The login, in order

1. The client connects and TLS runs.
2. The server sends **Hello**.
3. The client sends **SecretWord**, which is `potato` for now.  It is a filter for port scanners and bots, not security.  It may change with the date later.
4. The server sends **AwaitingAuthentication**.
5. The client sends **AuthenticationRequest**.  The password goes as typed, in plain text, which is safe because it is inside TLS.  The client doesn't hash it.
6. The server sends **AuthenticationResult**.  It never arrives sooner than 150 ms after the request, right or wrong, so the answer's timing says nothing about whether the name exists.
7. On a success, the server sends the **CharacterList** straight after, without being asked.  The player is in character select.

On any failure the server sends an AuthenticationResult with result 1 and closes the connection.  A failure is any of these:

- the wrong secret word
- a wrong username or password
- a packet out of order, or of a type the Login group doesn't have
- a packet that can't be read
- not sending the AuthenticationRequest within 10 seconds of the connection (TLS included, and counted after any hold below)

After a failure, that IP address waits 2 seconds from the failure before its next connection gets anywhere.  The server holds the new connection open and silent until then.  A client doesn't need to do anything about it except not give up in under 2 seconds.

### Already logged in

An account is only ever on once.  If the password is right but the account is already logged in somewhere else, the AuthenticationResult comes back with result 2 instead, and the client asks the player what to do.  It answers with a **SessionChoice**:

- **0, log the other session out.**  The other connection gets **LoggedOutElsewhere** and is closed.  This one gets result 0, "Welcome to Stratum.", and its CharacterList, the same as a normal login.
- **1, disconnect.**  The server closes this connection and leaves the other one alone.  That one is for shared accounts, so you don't kick your brother off because you wanted to play.

Result 2 only ever comes after the right password, so it tells a stranger nothing.  The player gets 30 seconds to choose.  Choosing "disconnect", running out the 30 seconds, or sending anything but a SessionChoice closes the connection with no answer and no 2 second hold, because none of those is a failed login.

## Character select

Once in, the player sees their characters and the empty slots, and can make a character, delete one, or pick one to play.

- **CreateCharacter** and **DeleteCharacter** each get a **CharacterResult** back.  On a success it is followed by a fresh CharacterList, so the client never has to work out the new list itself.  On a failure the message says why, in words the client can show as they are ("There is already a character called Aldric.").  Either way the connection stays open.
- **EnterWorld** gets a **WorldTicket** back if the character can be played, and a CharacterResult with result 1 if it can't.

The server decides what characters an account has.  An account has 3 slots for now, and the server refuses a fourth character whatever the client shows.  The slot count rides in the CharacterList, so a client never needs it written in.

Names go out the way the player should see them ("Aldric").  A name the client sends back can be in any case.  A character name is 4 to 12 letters, and no two characters on the server share one, whichever accounts they are on.

Deleting takes only the name.  The client makes the player type the name out to confirm before it sends the packet.  The server doesn't ask for the password again.

Once the player has a WorldTicket, character select is over for that connection.  Anything from the CharacterSelect group after that is ignored (and logged).  The connection stays open for chat and the fallback.

SimpleTcpMesg works any time after the login.  Packets the server doesn't take at that point are ignored and logged, not refused.  A packet it can't read closes the connection.

## The packets, one at a time

### 0x10 Hello

Server to client, as soon as TLS is up.  No payload.  "Say the secret word."

```text
01 00 00 00  10
```

### 0x11 SecretWord

Client to server.  One string.  `potato`:

```text
0B 00 00 00  11  06 00 00 00  70 6F 74 61 74 6F
```

### 0x12 AwaitingAuthentication

Server to client, after the right secret word.  No payload.  "Now the username and password."

```text
01 00 00 00  12
```

### 0x13 AuthenticationRequest

Client to server.  Two strings: the username, then the password as typed.

### 0x14 AuthenticationResult

Server to client.  A `u8` result, then a string for the player.

| Result | Meaning | Message |
|---|---|---|
| 0 | in | "Welcome to Stratum." |
| 1 | AUTHENTICATION FAILED | "Invalid Credentials", whatever went wrong |
| 2 | ALREADY LOGGED IN | "This account is already logged in." |

A failure:

```text
19 00 00 00  14  01  13 00 00 00  49 6E 76 61 6C 69 64 20 43 72 65 64 65 6E 74 69 61 6C 73
```

That is a length of 25: the type, the result byte, 4 for the string's length, and 19 for "Invalid Credentials".

Already logged in:

```text
28 00 00 00  14  02  22 00 00 00  54 68 69 73 20 61 63 63 6F 75 6E 74 20 69 73 20
                                  61 6C 72 65 61 64 79 20 6C 6F 67 67 65 64 20 69 6E 2E
```

### 0x15 SessionChoice

Client to server, only after an AuthenticationResult of 2.  One `u8`: 0 logs the other session out, 1 disconnects.  Anything else closes the connection.  Log the other one out:

```text
02 00 00 00  15  00
```

### 0x16 LoggedOutElsewhere

Server to client, on the connection that just got logged out from somewhere else.  No payload, and the server closes the connection straight after.  The client can tell the player why they were dropped.

```text
01 00 00 00  16
```

### 0x20 CharacterList

Server to client.  A `u8` for how many slots the account has, a `u8` for how many characters are in them, then each character's name as a string, in the order they were made.  3 slots, with Pooper and Poopy in two of them:

```text
16 00 00 00  20  03  02  06 00 00 00  50 6F 6F 70 65 72  05 00 00 00  50 6F 6F 70 79
```

An account with no characters still gets a list: the slots, and a count of 0.

```text
03 00 00 00  20  03  00
```

### 0x21 CreateCharacter

Client to server.  One string: the new character's name.  Aldric:

```text
0B 00 00 00  21  06 00 00 00  41 6C 64 72 69 63
```

### 0x22 DeleteCharacter

Client to server.  One string: the name of the character to delete.  The same shape as CreateCharacter, with 22 for the type.

### 0x23 EnterWorld

Client to server.  One string: the name of the character to play.  The same shape again, with 23.

### 0x24 CharacterResult

Server to client, after a CreateCharacter or a DeleteCharacter, and after an EnterWorld that didn't work.  A `u8` result (0 done, 1 refused), then a string for the player.  Aldric made:

```text
12 00 00 00  24  00  0C 00 00 00  41 6C 64 72 69 63 20 6D 61 64 65 2E
```

### 0x25 WorldTicket

Server to client, after an EnterWorld that worked.  The login token (a string: 64 lowercase hex characters), then the UDP port to take it to (a `u16`).  The port rides along so a client never has to be told it separately.

```text
47 00 00 00  25  40 00 00 00  (the 64 characters of the token)  0E 27
```

That is a length of 71: the type, 4 for the string's length, 64 for the token, and 2 for the port.  `0E 27` is 9998.

The token is how the UDP side will know who a packet is from.  It is made when the player picks a character, one per account, and a new one replaces the old.  It lives in the server's memory and nowhere else: for now it dies when the TCP connection closes, and every one of them dies on a restart.  The UDP side doesn't exist yet, so there is nowhere to take it.

### 0xF0 SimpleTcpMesg

Either way, any time after the login.  One string.  The server sends it straight back, which is how we test that both directions work.