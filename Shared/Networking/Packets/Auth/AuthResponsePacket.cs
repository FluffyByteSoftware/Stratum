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
    /// Represents an authentication response packet containing session information and credentials returned by the
    /// server after successful authentication.
    /// </summary>
    public readonly struct AuthResponsePacket : IPacketWritable
    {
        public const uint TypeId = PacketIds.Auth.AuthResponse;

        /// <summary>
        /// Token provided by the server upon successful authentication, 
        /// used for session management and authorization in subsequent requests.
        /// </summary>
        public string SessionToken { get; }
        /// <summary>
        /// Address to the UdpEndpoint
        /// </summary>
        public string UdpEndpoint { get; }
        /// <summary>
        /// Private key that has been issued by the server.
        /// </summary>
        public string IssuedPrivateKey { get; }

        /// <summary>
        /// Initializes a new instance of the <see cref="AuthResponsePacket"/> 
        /// class with the specified authentication response data.
        /// </summary>
        /// <param name="sessionToken">The session token for the authenticated 
        /// session.</param>
        /// <param name="udpEndpoint">The UDP endpoint address.</param>
        /// <param name="issuedPrivateKey">The private key issued for secure 
        /// communication.</param>
        public AuthResponsePacket(
            string sessionToken,
            string udpEndpoint,
            string issuedPrivateKey)
        {
            SessionToken = sessionToken;
            UdpEndpoint = udpEndpoint;
            IssuedPrivateKey = issuedPrivateKey;
        }

        uint IPacketWritable.TypeId => TypeId;

        /// <summary>
        /// Serializes the object to the specified writer.
        /// </summary>
        /// <param name="writer">The writer to serialize the data to.</param>
        public void Serialize(NetDataWriter writer)
        {
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
        public static AuthResponsePacket Deserialize(NetDataReader reader)
        {
            try
            {
                var token = reader.GetString();
                var endpoint = reader.GetString();
                var privateKey = reader.GetString();

                return new AuthResponsePacket(token, endpoint, privateKey);
            }
            catch (InvalidPacketException)
            {
                throw;
            }
            catch(Exception ex)
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