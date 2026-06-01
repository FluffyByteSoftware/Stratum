/*
 * (AuthByKeyPacket.cs)
 *------------------------------------------------------------
 * Created - 5/28/2026 3:19:35 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using LiteNetLib.Utils;
using Stratum.Shared.Networking;


namespace Shared.Networking.Packets.Auth
{
    public readonly struct AuthByKeyPacket : IPacketWritable
    {
        /// <summary>
        /// Expected type id of the packet type.
        /// </summary>
        public const uint TypeId = PacketIds.Auth.AuthByKey;

        /// <summary>
        /// The expected length of a signature in bytes.
        /// </summary>
        public const int SignatureLength = 64;

        /// <summary>
        /// Gets the account identifier.
        /// </summary>
        public string AccountId { get; }
        /// <summary>
        /// Unix timestamp in milliseconds.
        /// </summary>
        public long UnixTimestampMs { get; }
        /// <summary>
        /// Gets the signature.
        /// </summary>
        public byte[] Signature { get; }
        
        /// <summary>
        /// Initializes a new instance of the <see cref="AuthByKeyPacket"/> 
        /// class with the specified account ID, timestamp, and signature.
        /// </summary>
        /// <param name="accountId">The account identifier.</param>
        /// <param name="unixTimestampMs">The Unix timestamp in milliseconds.</param>
        /// <param name="signature">The cryptographic signature.</param>
        public AuthByKeyPacket(
            string accountId,
            long unixTimestampMs,
            byte[] signature)
        {
            AccountId = accountId;
            UnixTimestampMs = unixTimestampMs;
            Signature = signature;
        }

        uint IPacketWritable.TypeId => TypeId;

        /// <summary>
        /// Serializes the account authentication data to the specified writer.
        /// </summary>
        /// <param name="writer">The network data writer to write the serialized data to.</param>
        public void Serialize(NetDataWriter writer)
        {
            writer.Put(AccountId);
            writer.Put(UnixTimestampMs);
            writer.PutBytesWithLength(Signature);
        }

        /// <summary>
        /// Deserializes an <see cref="AuthByKeyPacket"/> from the specified data reader.
        /// </summary>
        /// <param name="reader">The data reader containing the serialized packet data.</param>
        /// <returns>The deserialized authentication packet.</returns>
        /// <exception cref="InvalidPacketException">Thrown when the signature length is invalid or deserialization fails.</exception>
        public static AuthByKeyPacket Deserialize(NetDataReader reader)
        {
            try
            {
                var accountId = reader.GetString();
                var timestampMs = reader.GetLong();
                var signature = reader.GetBytesWithLength();

                if (signature.Length != SignatureLength)
                    throw new InvalidPacketException(
                        TypeId,
                        $"Signature length {signature.Length} " +
                        $"!= expected {SignatureLength}.");

                return new AuthByKeyPacket(accountId, timestampMs, signature);
            }
            catch (InvalidPacketException)
            {
                throw;
            }
            catch(Exception ex)
            {
                throw new InvalidPacketException(
                    TypeId,
                    "Failed to deserialize AuthByKeyPacket.",
                    ex);
            }
        }
    }
}


/*
 *------------------------------------------------------------
 * (AuthByKeyPacket.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */