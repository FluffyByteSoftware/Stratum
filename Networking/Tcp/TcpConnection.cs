/*
 * (TcpConnection.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 2:31:07 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using System.IO;
using System.Net.Sockets;
using System.Threading;
using Stratum.Shared.Networking;


namespace Networking.Tcp;

/// <summary>
/// Represents a TCP connection with support for secure disconnection and 
/// thread-safe resource management.
/// </summary>
/// <param name="id">The unique identifier for the connection.</param>
/// <param name="client">The TCP client instance.</param>
/// <param name="stream">The underlying network stream for communication.</param>
/// <param name="remoteEndPoint">The remote endpoint address.</param>
public sealed class TcpConnection(
    long id, TcpClient client, Stream stream, string remoteEndPoint)
{
    private const int DisconnectRequestedBit = 0x100;
    private const int ReasonMask = 0xFF;

    /// <summary>
    /// Gets the unique identifier.
    /// </summary>
    public long Id { get; } = id;
    /// <summary>
    /// Gets the remote endpoint.
    /// </summary>
    public string RemoteEndPoint { get; } = remoteEndPoint;

    /// <summary>
    /// Gets the TCP client.
    /// </summary>
    internal TcpClient Client { get; } = client;
    /// <summary>
    /// Gets the TCP underlying Stream.
    /// </summary>
    internal Stream Stream { get; } = stream;
    /// <summary>
    /// Synchronization lock for send operations.
    /// </summary>
    internal SemaphoreSlim SendLock { get; } = new(1, 1);

    // Flag and reason share one word so the read loop observes both in a
    // single read: bit 8 marks "requested", the low byte is the reason.
    private int _disconnect;
    private int _closed;

    /// <summary>
    /// Requests a secure disconnection with the specified reason using a thread-safe 
    /// atomic operation.
    /// </summary>
    /// <param name="reason">The reason for requesting the disconnection.</param>
    public void RequestDisconnect(SecureDisconnectReason reason)
    {
        Interlocked.Exchange(
            ref _disconnect, DisconnectRequestedBit | (byte)reason);
    }

    /// <summary>
    /// Is a disconnect requested?
    /// </summary>
    internal bool DisconnectRequested =>
        Volatile.Read(ref _disconnect) != 0;

    /// <summary>
    /// Is there a disconnect reason for this client?
    /// </summary>
    internal SecureDisconnectReason DisconnectReason =>
        (SecureDisconnectReason)(Volatile.Read(ref _disconnect) & ReasonMask);

    /// <summary>
    /// Closes the connection and releases associated resources. This method is 
    /// thread-safe and idempotent.
    /// </summary>
    /// <remarks>Exceptions thrown during disposal of Stream and Client are 
    /// suppressed. SendLock is intentionally not disposed to avoid race 
    /// conditions with in-flight send operations.</remarks>
    internal void Close()
    {
        if (Interlocked.Exchange(ref _closed, 1) != 0)
        {
            return;
        }

        // Stream and Client own OS handles and must be released. SendLock is
        // deliberately left undisposed: SemaphoreSlim only allocates a
        // disposable handle if AvailableWaitHandle is read (never here), so
        // disposing it would do nothing but open a teardown race against an
        // in-flight send's WaitAsync/Release.
        try { Stream.Dispose(); } catch { }
        try { Client.Dispose(); } catch { }
    }
}

/*
 *------------------------------------------------------------
 * (TcpConnection.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */