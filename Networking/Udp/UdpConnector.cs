/*
 * (UdpConnector.cs)
 *------------------------------------------------------------
 * Created - 7/4/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using LiteNetLib;
using LiteNetLib.Utils;
using SystemTools.Logger;

namespace Networking.Udp;

/// <summary>
/// Represents a UDP client built on LiteNetLib that dials a single remote
/// host and surfaces connection lifecycle and traffic as events for a
/// consumer to apply policy against.
/// </summary>
/// <remarks>
/// The dial-side counterpart to <see cref="UdpHost"/>. Same
/// <see cref="INetEventListener"/> implementation, same single dedicated
/// poll thread, same event surface — so a process wiring either side of a
/// peer relationship follows one pattern. The two are kept as separate
/// classes rather than unified because their intent differs: a host binds a
/// listening port and admits unknown peers via
/// <see cref="UdpHost.ConnectionRequested"/>; a connector binds an ephemeral
/// local port and targets exactly one known remote. Collapsing them would
/// force every consumer to reason about accept-vs-dial branches irrelevant
/// to its role.
/// <para>
/// This class is transport only. It carries no knowledge of what the
/// connection data <i>means</i> — the caller builds whatever bytes the
/// remote expects (for zone registration, an Ed25519-signed timestamp) and
/// hands them in. Keeping signing out of the connector preserves the layer
/// boundary: <c>Networking</c> stays free of any <c>SystemTools.Security</c>
/// dependency, and this transport primitive stays reusable by any consumer
/// with any connection-data scheme, not just zone registration.
/// </para>
/// <para>
/// Reconnection is likewise the consumer's concern, driven off
/// <see cref="PeerDisconnected"/> — the connector makes one dial attempt on
/// <see cref="Start"/> and does not retry on its own.
/// </para>
/// </remarks>
public sealed class UdpConnector : INetEventListener
{
    private const int PollIntervalMs = 15;

    private readonly string _remoteHost;
    private readonly int _remotePort;
    private readonly NetDataWriter _connectionData;
    private readonly NetManager _net;
    private readonly CancellationTokenSource _shutdownCts = new();
    private readonly CancellationToken _shutdownToken;

    private Thread? _pollThread;
    private int _started;
    private int _stopped;

    /// <summary>
    /// Raised when the dial-out connection is accepted and established.
    /// Fires on the polling thread.
    /// </summary>
    public event Action<NetPeer>? PeerConnected;

    /// <summary>
    /// Raised when the peer disconnects for any reason — including a
    /// rejected or timed-out dial attempt. Fires on the polling thread.
    /// </summary>
    public event Action<NetPeer, DisconnectInfo>? PeerDisconnected;

    /// <summary>
    /// Raised when a packet is received from the connected peer. Fires on
    /// the polling thread. The reader is owned by LiteNetLib and is valid
    /// only for the duration of the callback.
    /// </summary>
    public event Action<NetPeer, NetPacketReader>? PacketReceived;

    /// <summary>
    /// Initializes a new instance of the <see cref="UdpConnector"/> class
    /// targeting a single remote endpoint.
    /// </summary>
    /// <param name="remoteHost">The remote host to dial.</param>
    /// <param name="remotePort">The remote port to dial (1-65535).</param>
    /// <param name="connectionData">The bytes presented to the remote as the
    /// connection request, already built by the caller (e.g. a signed
    /// registration marker). The connector treats these as opaque.</param>
    /// <exception cref="ArgumentOutOfRangeException"><paramref
    /// name="remotePort"/> is less than 1 or greater than 65535.</exception>
    /// <exception cref="ArgumentException"><paramref name="remoteHost"/> is
    /// null or empty.</exception>
    /// <exception cref="ArgumentNullException"><paramref
    /// name="connectionData"/> is null.</exception>
    public UdpConnector(
        string remoteHost,
        int remotePort,
        NetDataWriter connectionData)
    {
        if (remotePort is < 1 or > 65535)
            throw new ArgumentOutOfRangeException(
                nameof(remotePort), "Port must be in the range 1-65535.");

        ArgumentException.ThrowIfNullOrEmpty(remoteHost);
        ArgumentNullException.ThrowIfNull(connectionData);

        _remoteHost = remoteHost;
        _remotePort = remotePort;
        _connectionData = connectionData;
        _net = new NetManager(this);
        _shutdownToken = _shutdownCts.Token;
        _net.IPv6Enabled = false;
    }

    /// <summary>
    /// Binds an ephemeral local port, starts the poll thread, and initiates
    /// the dial-out connection to the configured remote endpoint.
    /// </summary>
    /// <exception cref="InvalidOperationException">The connector is already
    /// started, or the underlying socket failed to start.</exception>
    public void Start()
    {
        if (Interlocked.Exchange(ref _started, 1) != 0)
            throw new InvalidOperationException(
                "Connector is already started.");

        // Parameterless Start binds an OS-assigned ephemeral port — correct
        // here: nothing needs to reach this socket by a known port, only the
        // reverse.
        if (!_net.Start())
            throw new InvalidOperationException(
                "Failed to start UDP socket on an ephemeral local port.");

        _pollThread = new Thread(PollLoop)
        {
            Name = $"UdpConnector:{_remoteHost}:{_remotePort}",
            IsBackground = true,
        };

        _pollThread.Start();

        // Verify this overload against LiteNetLib 2.1.4 source/IntelliSense:
        // the binary-connection-data dial is Connect(string, int,
        // NetDataWriter). If the IDE shows a different shape, this line
        // changes and nothing else does.
        _net.Connect(_remoteHost, _remotePort, _connectionData);

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
            $"Udp Connector dialing {_remoteHost}:{_remotePort}."));
    }

    /// <summary>
    /// Stops the connector, ending the poll loop and closing the socket.
    /// </summary>
    /// <remarks>Safe to call multiple times; subsequent calls return
    /// immediately.</remarks>
    /// <returns>A task that completes once the poll thread has joined and the
    /// socket is stopped.</returns>
    public async Task StopAsync()
    {
        if (Interlocked.Exchange(ref _stopped, 1) != 0)
            return;

        _shutdownCts.Cancel();

        Thread? thread = _pollThread;

        if (thread is not null)
        {
            try
            {
                await Task.Run(() => thread.Join()).ConfigureAwait(false);
            }
            catch
            {
            }
        }

        try
        {
            _net.Stop();
        }
        catch
        {
        }

        _shutdownCts.Dispose();
    }

    private void PollLoop()
    {
        while (!_shutdownToken.IsCancellationRequested)
        {
            try
            {
                _net.PollEvents();
            }
            catch (Exception ex)
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                    $"UDP poll error: {ex.Message}", ex));
            }

            Thread.Sleep(PollIntervalMs);
        }
    }

    void INetEventListener.OnConnectionRequest(ConnectionRequest request)
    {
        // A pure dialer never expects inbound requests; reject defensively
        // rather than admit traffic this class was not built to serve.
        request.Reject();
    }

    void INetEventListener.OnPeerConnected(NetPeer peer)
        => PeerConnected?.Invoke(peer);

    void INetEventListener.OnPeerDisconnected(
        NetPeer peer, DisconnectInfo disconnectInfo)
        => PeerDisconnected?.Invoke(peer, disconnectInfo);

    void INetEventListener.OnNetworkReceive(
        NetPeer peer,
        NetPacketReader reader,
        byte channelNumber,
        DeliveryMethod deliveryMethod)
    {
        try
        {
            PacketReceived?.Invoke(peer, reader);
        }
        finally
        {
            reader.Recycle();
        }
    }

    void INetEventListener.OnNetworkReceiveUnconnected(
        System.Net.IPEndPoint remoteEndPoint,
        NetPacketReader reader,
        UnconnectedMessageType messageType)
    {
        reader.Recycle();
    }

    void INetEventListener.OnNetworkError(
        System.Net.IPEndPoint endPoint,
        System.Net.Sockets.SocketError socketError)
    {
        Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
            $"UDP socket error from {endPoint}: {socketError}."));
    }

    void INetEventListener.OnNetworkLatencyUpdate(NetPeer peer, int latency)
    {
        // No latency handling required.
    }
}

/*
 *------------------------------------------------------------
 * (UdpConnector.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */