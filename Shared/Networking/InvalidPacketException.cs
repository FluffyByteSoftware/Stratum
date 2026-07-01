/*
 * (InvalidPacketException.cs)
 *------------------------------------------------------------
 * Created - 5/28/2026 2:57:45 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;

namespace Shared.Networking;

/// <summary>
/// Thrown when a packet with a specific type identifier is invalid.
/// </summary>
public sealed class InvalidPacketException : Exception
{
    /// <summary>
    /// Gets the unique identifier for the packet type.
    /// </summary>
    public uint TypeId { get; }

    /// <summary>
    /// Initializes a new instance of the <see cref="InvalidPacketException"/> 
    /// class with a specified packet type identifier and error message.
    /// </summary>
    /// <param name="typeId">The packet type identifier.</param>
    /// <param name="message">The message that describes the error.</param>
    public InvalidPacketException(uint typeId, 
        string message) : base(message)
    {
        TypeId = typeId;
    }

    /// <summary>
    /// Initializes a new instance of the <see cref="InvalidPacketException"/>
    /// class with a specified packet type identifier, error message, 
    /// and inner exception.</summary>
    /// <param name="typeId">The packet type identifier.</param>
    /// <param name="message">The error message that explains the reason for the exception.</param>
    /// <param name="inner">The exception that is the cause of the current exception.</param>
    public InvalidPacketException(
        uint typeId,
        string message,
        Exception inner) : base(message, inner)
    {
        TypeId = typeId;
    }
}


/*
*------------------------------------------------------------
* (InvalidPacketException.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/