/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 5/31/2026 11:04:43 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System;
using Networking.Dispatch;
using SystemTools.Accounts;
using SystemTools.Config;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;
using System.Net;
using System.Threading.Tasks;
using Shared.Networking.Packets.Auth;
using Networking.Tcp;
using SystemTools;

namespace LoginServer;

/// <summary>
/// The main boot operator for the LoginServer. 
/// Maintains the Tcp connection with clients.
/// </summary>
internal static class Program
{

    private static async Task Main()
    {
        DiskManager.Initialize();

        TcpHost? host = null;

        try
        {
            var serverConfig = ConfigStore.LoadOrCreate<ServerConfig>(Constellations.ServerConfigPath);
            var networkConfig = ConfigStore.LoadOrCreate<NetworkConfig>(Constellations.NetworkConfigPath);

            if (!IPAddress.TryParse(networkConfig.BindAddress, out var bindAddress))
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Invalid bindAddress in config: '{networkConfig.BindAddress}'."));

            var certificate = CertificateProvider.LoadOrCreate(Constellations.CertificatePath);

            AccountStore.Initialize();

            var signingKey = SessionKeyProvider.LoadOrCreate();
            SessionTokenIssuer.Initialize(signingKey);

            var lockout = new LoginServer.LockoutTracker();
            var udpHost = string.Concat(networkConfig.UdpAddress, ":", 
                networkConfig.UdpPort.ToString());

            var handlers = new AuthHandler(
                AccountStore.Instance, lockout, udpHost);

            var dispatcher = new PacketDispatcher<TcpConnection>();
            dispatcher.Register<AuthByKeyPacket>(
                AuthByKeyPacket.TypeId,
                AuthByKeyPacket.Deserialize,
                handlers.OnAuthByKey);

            dispatcher.Register<AuthByPasswordPacket>(
                AuthByPasswordPacket.TypeId,
                AuthByPasswordPacket.Deserialize,
                handlers.OnAuthByPassword);

            dispatcher.Freeze();

            bindAddress ??= IPAddress.Loopback;

            host = new TcpHost(
                bindAddress, networkConfig.Port, dispatcher, certificate);

            // Close the Host null-window before a connection arrives
            handlers.Host = host;

            host.Start();

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"{serverConfig.GMUDName} LoginServer ready on "
                + $"{networkConfig.BindAddress}:{networkConfig.Port}."));

            await WaitForShutdownAsync();
        }
        catch(Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "LoginServer terminated by an unhandled exception.", ex));
        }
        finally
        {
            if (host is not null)
                await host.StopAsync();

            if (AccountStore.IsRunning)
                await AccountStore.Instance.ShutdownAsync();

            await Scribe.ShutdownAsync();

            if(DiskManager.IsRunning)
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