/*
 * (CharacterCreateResponsePacket.cs)
 *------------------------------------------------------------
 * Created - 6/27/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using LiteNetLib.Utils;

namespace Shared.Networking.Packets.Character;

/// <summary>
/// The wire-level outcome of a character-create attempt, carried on
/// <see cref="CharacterCreateResponsePacket"/>.
/// </summary>
/// <remarks>
/// This enum is the <c>Shared</c>-side mirror of the server-internal
/// <c>SystemTools.Characters.CharacterCreateResult</c>. It must exist separately
/// because the create service and its result enum live in <c>SystemTools</c>
/// (net10.0), which the <c>Shared</c> wire library cannot reference — the
/// dependency runs <c>SystemTools → Shared</c>, never the reverse, and the Unity
/// client reads <c>Shared</c> but never <c>SystemTools</c>. The create handler
/// (in LoginServer, which sees both assemblies) maps one to the other explicitly.
/// The members mirror <c>CharacterCreateResult</c> one-for-one, by name and value,
/// today; the mapping is nonetheless an explicit switch rather than a numeric cast
/// so the two can diverge safely if either side ever gains a member — the wire
/// contract must not shift silently because a service-side enum was reordered.
/// <see cref="None"/> is the loud-failure sentinel per project convention: the
/// server never writes it, so reading it back signals an uninitialized or corrupt
/// packet, never a real outcome.
/// </remarks>
public enum CharacterCreateOutcome : byte
{
    /// <summary>
    /// Unset sentinel. Never written by the server; its presence indicates an
    /// uninitialized or corrupt packet.
    /// </summary>
    None = 0,

    /// <summary>
    /// The character was created and both the character file and the account
    /// back-reference were persisted. The client should re-authenticate to obtain
    /// its session token through the standard <c>Ok</c> path.
    /// </summary>
    Created = 1,

    /// <summary>
    /// The requested name failed validation (empty, wrong length, or contained
    /// anything other than letters). The player may retry with a different name.
    /// </summary>
    NameInvalid = 2,

    /// <summary>
    /// No account exists for the authenticated connection. Not expected in the
    /// normal flow — the connection authed against a real account — but carried
    /// for honesty against a race.
    /// </summary>
    AccountNotFound = 3,

    /// <summary>
    /// The account already owns a character. Not expected in the normal flow,
    /// which only reaches create on a <c>NeedsCharacter</c> outcome, but carried
    /// against a race where a character appeared between auth and create.
    /// </summary>
    AccountAlreadyHasCharacter = 4,

    /// <summary>
    /// A character with the requested canonical name already exists. Names are
    /// globally unique; the player may retry with a different name.
    /// </summary>
    NameTaken = 5,

    /// <summary>
    /// A disk write failed server-side. Not the player's fault and not retryable
    /// by changing the name; the account's link self-heals on the next Core boot.
    /// </summary>
    PersistFailed = 6,
}

/// <summary>
/// Server-to-client response reporting the outcome of a character-create attempt,
/// sent back on the LoginServer TCP connection the request arrived on.
/// </summary>
/// <remarks>
/// The packet carries only the <see cref="CharacterCreateOutcome"/> — no session
/// token or endpoint. On <see cref="CharacterCreateOutcome.Created"/> the server
/// sends this response and then disconnects; the client re-authenticates, and
/// because the account now owns a character, the existing <c>Ok</c> branch mints
/// the token through the single, already-verified issuance path. Folding a token
/// into this response would duplicate that issuance into the create handler to
/// save one round-trip on a once-per-account operation — not a trade worth a second
/// mint site. The struct is a dumb carrier: it does not police which outcomes are
/// legal here; producing a coherent value is the sending handler's job.
/// </remarks>
/// <remarks>
/// Initializes a new instance of the
/// <see cref="CharacterCreateResponsePacket"/> struct.
/// </remarks>
/// <param name="outcome">The outcome of the create attempt.</param>
public readonly struct CharacterCreateResponsePacket(CharacterCreateOutcome outcome) : IPacketWritable
{
    /// <summary>
    /// The packet type id on the character channel.
    /// </summary>
    public const uint TypeId = MessagePacketIds.CharacterMessage.CharacterCreateResponse;

    /// <summary>
    /// The outcome of the create attempt the client switches on.
    /// </summary>
    public CharacterCreateOutcome Outcome { get; } = outcome;

    uint IPacketWritable.TypeId => TypeId;

    /// <summary>
    /// Serializes the outcome to the specified writer.
    /// </summary>
    /// <param name="writer">The writer to serialize the data to.</param>
    public void Serialize(NetDataWriter writer)
    {
        writer.Put((byte)Outcome);
    }

    /// <summary>
    /// Deserializes a <see cref="CharacterCreateResponsePacket"/> from the
    /// specified reader.
    /// </summary>
    /// <param name="reader">The reader containing the serialized packet
    /// data.</param>
    /// <returns>The deserialized
    /// <see cref="CharacterCreateResponsePacket"/>.</returns>
    /// <exception cref="InvalidPacketException">Thrown when deserialization
    /// fails.</exception>
    /// <remarks>
    /// The leading byte is cast directly to <see cref="CharacterCreateOutcome"/>
    /// with no clamp of unknown values to <see cref="CharacterCreateOutcome.None"/>.
    /// Like <c>AuthResponsePacket</c>, this is a server→client packet on a
    /// connection the client has already TLS-authenticated, so a hostile
    /// discriminant is not a threat this path defends against.
    /// </remarks>
    public static CharacterCreateResponsePacket Deserialize(NetDataReader reader)
    {
        try
        {
            var outcome = (CharacterCreateOutcome)reader.GetByte();

            return new CharacterCreateResponsePacket(outcome);
        }
        catch (InvalidPacketException)
        {
            throw;
        }
        catch (Exception ex)
        {
            throw new InvalidPacketException(
                TypeId,
                "Failed to deserialize CharacterCreateResponsePacket.",
                ex);
        }
    }
}


/*
*------------------------------------------------------------
* (CharacterCreateResponsePacket.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/