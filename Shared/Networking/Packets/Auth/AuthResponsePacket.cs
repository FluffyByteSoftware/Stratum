/*
 * (AuthResponsePacket.cs)
 *------------------------------------------------------------
 * Created - 5/29/2026 9:02:41 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using LiteNetLib.Utils;
using Stratum.Shared.Networking;

namespace Shared.Networking.Packets.Auth
{
    /// <summary>
    /// The result of the login→world decision, carried on every
    /// <see cref="AuthResponsePacket"/> so the client knows how to interpret the
    /// rest of the packet.
    /// </summary>
    /// <remarks>
    /// After TLS authentication succeeds, LoginServer reads the account's
    /// <c>CharacterName</c> back-reference to decide whether the player already owns a
    /// playable character. That single decision is what this enum encodes, and it is
    /// the discriminant the client switches on before reading the conditional fields:
    /// on <see cref="Ok"/> the session token and UDP endpoint are meaningful and the
    /// player proceeds to Sentinel; on <see cref="NeedsCharacter"/> they are empty and
    /// the client must drive character creation first. <see cref="None"/> is the
    /// zero-value loud-failure sentinel in keeping with project convention — the server
    /// never writes it, so reading it back means an uninitialized or corrupt packet,
    /// never a valid outcome.
    /// </remarks>
    public enum AuthOutcome : byte
    {
        /// <summary>
        /// Unset sentinel. Never written by the server; its presence on the wire
        /// indicates an uninitialized or corrupt packet.
        /// </summary>
        None = 0,

        /// <summary>
        /// Authentication succeeded and the account owns a playable character. The
        /// session token and UDP endpoint are populated and the client proceeds to
        /// Sentinel.
        /// </summary>
        Ok = 1,

        /// <summary>
        /// Authentication succeeded but the account has no character yet. The session
        /// token and UDP endpoint are empty; the client must complete character
        /// creation before a token will be issued. Any freshly issued private key
        /// still rides along so a brand-new password account keeps its minted key.
        /// </summary>
        NeedsCharacter = 2,
    }

    /// <summary>
    /// Represents an authentication response packet containing the login→world outcome
    /// and the session information returned by the server after successful
    /// authentication.
    /// </summary>
    /// <remarks>
    /// The packet always carries an <see cref="AuthOutcome"/> discriminant; the
    /// remaining fields are conditional on it. On <see cref="AuthOutcome.Ok"/> the
    /// <see cref="SessionToken"/> and <see cref="UdpEndpoint"/> are populated and the
    /// client hands the token to Sentinel. On <see cref="AuthOutcome.NeedsCharacter"/>
    /// those two are empty strings — the account has no character, and no token is
    /// minted until one exists — while <see cref="IssuedPrivateKey"/> may still be set
    /// for a first-time password login. The struct is a dumb data carrier: it does not
    /// enforce which fields are populated for which outcome; constructing a coherent
    /// combination is the sending handler's responsibility.
    /// </remarks>
    public readonly struct AuthResponsePacket : IPacketWritable
    {
        public const uint TypeId = PacketIds.Auth.AuthResponse;

        /// <summary>
        /// The login→world outcome. The client switches on this before reading the
        /// conditional fields below.
        /// </summary>
        public AuthOutcome Outcome { get; }

        /// <summary>
        /// Token provided by the server upon successful authentication, 
        /// used for session management and authorization in subsequent requests.
        /// Empty unless <see cref="Outcome"/> is <see cref="AuthOutcome.Ok"/>.
        /// </summary>
        public string SessionToken { get; }
        /// <summary>
        /// Address to the UdpEndpoint.
        /// Empty unless <see cref="Outcome"/> is <see cref="AuthOutcome.Ok"/>.
        /// </summary>
        public string UdpEndpoint { get; }
        /// <summary>
        /// Private key that has been issued by the server.
        /// </summary>
        public string IssuedPrivateKey { get; }

        /// <summary>
        /// Initializes a new instance of the <see cref="AuthResponsePacket"/> 
        /// struct with the specified outcome and authentication response data.
        /// </summary>
        /// <param name="outcome">The login→world outcome the client switches on.</param>
        /// <param name="sessionToken">The session token for the authenticated 
        /// session; pass an empty string when no token is issued.</param>
        /// <param name="udpEndpoint">The UDP endpoint address; pass an empty string
        /// when no token is issued.</param>
        /// <param name="issuedPrivateKey">The private key issued for secure 
        /// communication; pass an empty string when none was minted.</param>
        public AuthResponsePacket(
            AuthOutcome outcome,
            string sessionToken,
            string udpEndpoint,
            string issuedPrivateKey)
        {
            Outcome = outcome;
            SessionToken = sessionToken;
            UdpEndpoint = udpEndpoint;
            IssuedPrivateKey = issuedPrivateKey;
        }

        uint IPacketWritable.TypeId => TypeId;

        /// <summary>
        /// Serializes the object to the specified writer.
        /// </summary>
        /// <param name="writer">The writer to serialize the data to.</param>
        /// <remarks>
        /// The single <see cref="AuthOutcome"/> byte leads the payload so the client
        /// reads the discriminant before the three result-dependent strings. The type
        /// id is unchanged; only the payload grows by this one leading byte, which is
        /// why the Probe regression must be re-run after this change.
        /// </remarks>
        public void Serialize(NetDataWriter writer)
        {
            writer.Put((byte)Outcome);
            writer.Put(SessionToken);
            writer.Put(UdpEndpoint);
            writer.Put(IssuedPrivateKey);
        }

        /// <summary>
        /// Deserializes an <see cref="AuthResponsePacket"/> from the specified reader.
        /// </summary>
        /// <param name="reader">The reader containing the serialized packet data.</param>
        /// <returns>The deserialized <see cref="AuthResponsePacket"/>.</returns>
        /// <exception cref="InvalidPacketException">Thrown when the packet data is 
        /// invalid or deserialization fails.</exception>
        /// <remarks>
        /// The leading byte is cast directly to <see cref="AuthOutcome"/> with no
        /// clamp of unknown values to <see cref="AuthOutcome.None"/>. This is a
        /// server→client packet over a connection the client has already TLS-
        /// authenticated, so a hostile or malformed discriminant is not a threat this
        /// path defends against; an unrecognized value simply reads as an undefined
        /// enum member and is the caller's to handle.
        /// </remarks>
        public static AuthResponsePacket Deserialize(NetDataReader reader)
        {
            try
            {
                var outcome = (AuthOutcome)reader.GetByte();
                var token = reader.GetString();
                var endpoint = reader.GetString();
                var privateKey = reader.GetString();

                return new AuthResponsePacket(
                    outcome, token, endpoint, privateKey);
            }
            catch (InvalidPacketException)
            {
                throw;
            }
            catch (Exception ex)
            {
                throw new InvalidPacketException(
                    TypeId,
                    "Failed to deserialize AuthResponsePacket.",
                    ex);
            }
        }
    }
}


/*
 *------------------------------------------------------------
 * (AuthResponsePacket.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */