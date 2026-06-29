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