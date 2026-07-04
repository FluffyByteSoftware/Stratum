/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 7/4/2026 11:12:50 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using System.Threading.Tasks;
using Networking.Udp;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;

namespace ZoneManager;

/// <summary>
/// Entry point for the ZoneManager process: the star-topology hub the
/// Zone processes dial outward to and register against over UDP. This
/// skeleton stands up the registration transport and idles; the
/// registration verify path, zone registry, and death clock arrive in
/// later bricks.
/// </summary>
/// <remarks>
/// Boot order mirrors LoginServer and Sentinel: <see cref="DiskManager"/>
/// is initialized first so every downstream storage call has a live disk
/// layer, and teardown in the <c>finally</c> reverses it — stop the host,
/// drain <see cref="Scribe"/>, then flush and shut down the disk layer
/// last so any final writes are persisted.
/// </remarks>
internal static class Program
{
    /// <summary>
    /// Loopback bind port for the registration <see cref="UdpHost"/>.
    /// Hardcoded for the single-box dev skeleton — nothing dials it yet,
    /// so no cross-process agreement is needed. Graduates to a
    /// <c>NetworkConfig</c> field (a shared-tier fork) when the Zone dial
    /// side must discover this port to register, next brick.
    /// </summary>
    private const int UdpPort = 9050;

    /// <summary>
    /// The zone-registration public key ZoneManager verifies signatures
    /// against. Loaded at boot to exercise the provider's first real call
    /// — which generates and clones the keypair on genuine first boot —
    /// and held for the verify path that reads it in the registration
    /// round trip.
    /// </summary>
    private static byte[]? _registrationPublicKey;

    private static UdpHost? _host;

    private static async Task Main()
    {
        DiskManager.Initialize();

        try
        {
            _registrationPublicKey =
                ZoneRegistrationKeyProvider.LoadOrCreatePublic();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                "Zone registration public key loaded "
                + $"({_registrationPublicKey.Length} bytes)."));

            _host = new UdpHost(UdpPort, UdpBindMode.LoopbackOnly);
            _host.Start();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                "ZoneManager ready. UDP registration listening on "
                + $"loopback port {UdpPort}."));

            await WaitForShutdownAsync();
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "ZoneManager terminated by an unhandled exception.", ex));
        }
        finally
        {
            if (_host is not null)
                await _host.StopAsync();

            await Scribe.ShutdownAsync();

            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync();
        }
    }

    private static Task WaitForShutdownAsync()
    {
        var tcs = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);

        Console.CancelKeyPress += (_, e) =>
        {
            // Suppress the default kill so the finally teardown can run.
            e.Cancel = true;
            tcs.TrySetResult();
        };

        return tcs.Task;
    }
}

/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */