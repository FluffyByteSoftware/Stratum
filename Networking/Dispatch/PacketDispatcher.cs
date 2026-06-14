/*
 * (PacketDispatcher.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 7:21:21 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using System.Collections.Frozen;
using System.Collections.Generic;
using System.Threading.Tasks;
using LiteNetLib.Utils;
using Stratum.Networking.Dispatch;

namespace Networking.Dispatch;

/// <summary>
/// Represents a method that deserializes a packet from a data reader.
/// </summary>
/// <typeparam name="TPacket">The type of packet to deserialize.</typeparam>
/// <param name="reader">The data reader containing the serialized packet data.</param>
/// <returns>The deserialized packet.</returns>
public delegate TPacket PacketDeserializer<out TPacket>(NetDataReader reader);

/// <summary>
/// Represents an asynchronous handler for processing packets received on a connection.
/// </summary>
/// <typeparam name="TConnection">The type of the connection.</typeparam>
/// <typeparam name="TPacket">The type of the packet to handle.</typeparam>
/// <param name="connection">The connection on which the packet was received.</param>
/// <param name="packet">The packet to process.</param>
/// <returns>A <see cref="ValueTask"/> representing the asynchronous packet handling
/// operation.</returns>
public delegate ValueTask PacketHandler<in TConnection, in TPacket>(
    TConnection connection, TPacket packet);

/// <summary>
/// Manages registration and dispatching of typed packet handlers based on packet type 
/// identifiers.
/// </summary>
/// <remarks>Packet handlers must be registered using <see cref="Register{TPacket}(uint,
/// PacketDeserializer{TPacket}, PacketHandler{TConnection, TPacket})"/> before calling 
/// <see cref="Freeze"/>. Once frozen, the dispatcher becomes immutable and can be 
/// used to dispatch packets via <see cref="DispatchAsync(TConnection, 
/// uint, NetDataReader)"/>.</remarks>
/// <typeparam name="TConnection">The type of connection that will be passed to packet 
/// handlers.</typeparam>
public sealed class PacketDispatcher<TConnection>
{
    private delegate ValueTask<DispatchResult> ErasedDispatch(
        TConnection connection, NetDataReader reader);

    private readonly Dictionary<uint, ErasedDispatch> _registrations = [];
    private FrozenDictionary<uint, ErasedDispatch>? _frozen;
    private bool _isFrozen;

    /// <summary>
    /// Indicates whether the dispatcher is frozen or unlocked.
    /// </summary>
    public bool IsFrozen => _isFrozen;
    
    /// <summary>
    /// Registers a packet type with its deserializer and handler.
    /// </summary>
    /// <typeparam name="TPacket">The packet type to register.</typeparam>
    /// <param name="typeId">The unique identifier for the packet type.</param>
    /// <param name="deserialize">The function to deserialize the packet from a 
    /// reader.</param>
    /// <param name="handler">The function to handle the deserialized packet.</param>
    /// <exception cref="InvalidOperationException">Thrown when the dispatcher is 
    /// frozen or when <paramref name="typeId"/> is already registered.</exception>
    public void Register<TPacket>(
        uint typeId,
        PacketDeserializer<TPacket> deserialize,
        PacketHandler<TConnection, TPacket> handler)
    {
        if (_isFrozen)
            throw new InvalidOperationException(
                "Cannot register after the dispatcher is frozen.");

        if (_registrations.ContainsKey(typeId))
            throw new InvalidOperationException(
                $"Type 0x{typeId:X8} is already registered.");

        _registrations[typeId] = async (connection, reader) =>
        {
            TPacket packet;
            try
            {
                packet = deserialize(reader);
            }
            catch (Exception ex)
            {
                return DispatchResult.InvalidPacket(typeId, ex);
            }

            try
            {
                await handler(connection, packet).ConfigureAwait(false);
                return DispatchResult.Success(typeId);
            }
            catch (Exception ex)
            {
                return DispatchResult.HandlerException(typeId, ex);
            }
        };
    }

    /// <summary>
    /// Freezes the dispatcher, making its registrations immutable.
    /// </summary>
    /// <exception cref="InvalidOperationException">The dispatcher is already frozen.</exception>
    public void Freeze()
    {
        if (_isFrozen)
            throw new InvalidOperationException(
                "Dispatcher is already frozen.");

        _frozen = _registrations.ToFrozenDictionary();
        _isFrozen = true;
    }

    /// <summary>
    /// Dispatches an incoming message to the appropriate handler based on the type identifier.
    /// </summary>
    /// <param name="connection">The connection from which the message was received.</param>
    /// <param name="typeId">The type identifier of the message to dispatch.</param>
    /// <param name="reader">The data reader containing the message payload.</param>
    /// <returns>A task representing the asynchronous dispatch operation, containing the dispatch result.</returns>
    /// <exception cref="InvalidOperationException">The dispatcher has not been frozen.</exception>
    public ValueTask<DispatchResult> DispatchAsync(
        TConnection connection, uint typeId, NetDataReader reader)
    {
        if (!_isFrozen)
            throw new InvalidOperationException(
                "Dispatcher must be frozen before dispatch.");

        if(!_frozen!.TryGetValue(typeId, out var dispatch))
        {
            return new ValueTask<DispatchResult>(
                DispatchResult.UnknownType(typeId));
        }

        return dispatch(connection, reader);
    }
}



/*
 *------------------------------------------------------------
 * (PacketDispatcher.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */