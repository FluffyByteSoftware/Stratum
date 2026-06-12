/*
 * (PacketFramer.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 7:53:22 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Buffers.Binary;

namespace Networking.Tcp;

/// <summary>
/// Provides methods for framing and parsing binary packets with fixed 8-byte headers.
/// </summary>
/// <remarks>Each frame consists of an 8-byte header followed by the payload data. 
/// The header contains a 4-byte big-endian integer representing the payload length, 
/// followed by a 4-byte big-endian unsigned integer representing the type identifier.
/// </remarks>
public static class PacketFramer
{
    public const int HeaderSize = 8;
    public const int MaxFrameBytes = 1024 * 1024; // 1 MiB

    /// <summary>
    /// Expected frame size
    /// </summary>
    /// <param name="payloadLength">Payload length to process through</param>
    /// <returns>A total integer value of the expected frame size.</returns>
    public static int FrameSize(int payloadLength) =>
        HeaderSize + payloadLength;

    /// <summary>
    /// Writes a framed packet consisting of a length prefix, type identifier, and 
    /// payload to the destination buffer.
    /// </summary>
    /// <param name="typeId">The frame type identifier.</param>
    /// <param name="payload">The payload data to include in the frame.</param>
    /// <param name="destination">The buffer to write the framed packet to.</param>
    /// <returns>The total number of bytes written to the destination.</returns>
    /// <exception cref="ArgumentOutOfRangeException">The payload length exceeds 
    /// <see cref="MaxFrameBytes"/>.</exception>
    /// <exception cref="ArgumentException">The destination buffer is too small to 
    /// hold the framed packet.</exception>
    public static int WriteFrame(
        uint typeId, ReadOnlySpan<byte> payload,
        Span<byte> destination)
    {
        if (payload.Length > MaxFrameBytes)
            throw new ArgumentOutOfRangeException(
                nameof(payload),
                $"Payload of {payload.Length} bytes exceeds the " +
                $"{MaxFrameBytes}-byte limit.");

        int frameSize = HeaderSize + payload.Length;

        if (destination.Length < frameSize)
            throw new ArgumentException(
                $"Destination is too small for the framed packet.",
                nameof(destination));

        BinaryPrimitives.WriteInt32BigEndian(destination, payload.Length);
        BinaryPrimitives.WriteUInt32BigEndian(destination[4..], typeId);
        payload.CopyTo(destination[HeaderSize..]);

        return frameSize;
    }

    /// <summary>
    /// Attempts to read and validate a message header from a byte span.
    /// </summary>
    /// <param name="header">The byte span containing the header data to read.</param>
    /// <param name="typeId">When this method returns, contains the type identifier 
    /// from the header if successful; otherwise, 0.</param>
    /// <param name="payloadLength">When this method returns, contains the payload length 
    /// from the header if successful; otherwise, 0.</param>
    /// <returns><see langword="true"/> if the header was successfully read and validated; 
    /// otherwise, <see langword="false"/>.</returns>
    public static bool TryReadHeader(
        ReadOnlySpan<byte> header, 
        out uint typeId, 
        out int payloadLength)
    {
        typeId = 0;
        payloadLength = 0;

        if (header.Length < HeaderSize)
            return false;

        int length = BinaryPrimitives.ReadInt32BigEndian(header);
        
        if (length < 0 || length > MaxFrameBytes)
            return false;

        typeId = BinaryPrimitives.ReadUInt32BigEndian(header[4..]);
        payloadLength = length;

        return true;
    }
}



/*
 *------------------------------------------------------------
 * (PacketFramer.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */