/*
 * (AuthByPasswordPacket.cs)
 *------------------------------------------------------------
 * Created - 5/28/2026 7:05:58 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using LiteNetLib.Utils;

namespace Stratum.Shared.Networking.Packets.Auth
{
    public readonly struct AuthByPasswordPacket : IPacketWritable
    {
        public const uint TypeId = PacketIds.Auth.AuthByPassword;

        /// <summary>
        /// The account ID of the packet.
        /// </summary>
        public string AccountId { get; }
        /// <summary>
        /// The password for this account of this packet.
        /// </summary>
        public string Password { get; }

        /// <summary>
        /// Initializes a new instance of the <see cref="AuthByPasswordPacket"/> class.
        /// </summary>
        /// <param name="accountId">The account identifier.</param>
        /// <param name="password">The password.</param>
        public AuthByPasswordPacket(string accountId, string password)
        {
            AccountId = accountId;
            Password = password;
        }

        uint IPacketWritable.TypeId => TypeId;

        /// <summary>
        /// Serializes the account credentials to the specified writer.
        /// </summary>
        /// <param name="writer">The writer to serialize the data to.</param>
        public void Serialize(NetDataWriter writer)
        {
            writer.Put(AccountId);
            writer.Put(Password);
        }

        /// <summary>
        /// Deserializes an AuthByPasswordPacket from a NetDataReader.
        /// </summary>
        /// <param name="reader">The data reader containing the serialized packet 
        /// data.</param>
        /// <returns>A new AuthByPasswordPacket instance created from the deserialized 
        /// data.</returns>
        /// <exception cref="InvalidPacketException">Thrown when the packet data is 
        /// invalid or deserialization fails.</exception>
        public static AuthByPasswordPacket Deserialize(NetDataReader reader)
        {
            try
            {
                var accountId = reader.GetString();
                var password = reader.GetString();

                return new AuthByPasswordPacket(accountId, password);
            }
            catch (InvalidPacketException)
            {
                throw;
            }
            catch(Exception ex)
            {
                throw new InvalidPacketException(
                    TypeId,
                    "Failed to deserialize AuthByPasswordPacket.",
                    ex);
            }
        }
    }
}

/*
 *------------------------------------------------------------
 * (AuthByPasswordPacket.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */