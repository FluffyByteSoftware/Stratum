/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 6/3/2026 7:42:47 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Accounts;
using SystemTools.Characters;
using SystemTools.Logger;
using SystemTools.Storage;
using Game.Living.Characters;

namespace Core;

internal static class Program
{
    private static bool ServerRunning =>
        LoginServerProcess.IsRunning || SentinelProcess.IsRunning;

    private static async Task<int> Main()
    {
        DiskManager.Initialize();
        AccountStore.Initialize();
        CharacterStore.Initialize();
        SpeciesStore.Initialize();
        
        ReconcileLinks();

        ResolveCharacterSpecies();

        try
        {
            await RunMenuAsync().ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "Unhandled exception in Core.", ex));
            Console.WriteLine($"Error: {ex.Message}");
        }
        finally
        {
            if (CharacterStore.IsRunning)
                await CharacterStore.Instance.ShutdownAsync()
                    .ConfigureAwait(false);

            if (AccountStore.IsRunning)
                await AccountStore.Instance.ShutdownAsync().ConfigureAwait(false);

            await Scribe.ShutdownAsync().ConfigureAwait(false);

            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync().ConfigureAwait(false);
        }

        return 0;
    }

    /// <summary>
    /// Runs the account-character reconciler once at startup, after both stores
    /// have loaded, and surfaces the summary to the operator and the durable log.
    /// </summary>
    /// <remarks>
    /// Core is the system-admin console and the natural place to report link
    /// health: a clean run is an informational one-liner, while anomalies are
    /// raised to <see cref="ScribeSeverity.Warn"/> and direct the admin to the log,
    /// where the reconciler has already written the per-case detail. The reconciler
    /// runs here, outside any store's <c>Initialize</c>, because it depends on both
    /// stores being loaded — a dependency a single store cannot satisfy from inside
    /// its own boot.
    /// </remarks>
    private static void ReconcileLinks()
    {
        var report = AccountCharacterReconciler.Run();

        if (report.Anomalies == 0)
        {
            Console.WriteLine(
                $"Account-character links reconciled: "
                    + $"{report.Healthy} healthy, {report.Healed} healed.");

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Reconcile clean: {report.Healthy} healthy, "
                    + $"{report.Healed} healed, 0 anomalies."));

            return;
        }

        Console.WriteLine(
            $"Account-character reconcile found {report.Anomalies} "
                + $"anomalies ({report.Healthy} healthy, {report.Healed} "
                + "healed). See the server log for detail.");

        Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
            $"Reconcile found {report.Anomalies} anomalies: "
                + $"{report.Healthy} healthy, {report.Healed} healed. "
                + "Per-case detail logged above."));
    }

    private static async Task RunMenuAsync()
    {
        while (true)
        {
            PrintMenu();

            var choice = Console.ReadLine()?.Trim();
            switch (choice)
            {
                case "1":
                    StartStopServer();
                    break;
                case "2":
                    CheckResources();
                    break;
                case "3":
                    await PromptAndRunAsync("Account id to create",
                        AccountManager.CreateAccountAsync).ConfigureAwait(false);
                    break;
                case "4":
                    await PromptAndRunAsync("Account id to reset",
                        AccountManager.ResetPasswordAsync).ConfigureAwait(false);
                    break;
                case "5":
                    await PromptAndRunAsync("Account id to delete",
                        AccountManager.DeleteAccountAsync).ConfigureAwait(false);
                    break;
                case "6":
                    AccountManager.ListAccounts();
                    break;
                case "0":
                case "quit":
                case "exit":
                    StopAll();
                    return;
                default:
                    Console.WriteLine($"Unknown option: '{choice}'.");
                    break;
            }
            Console.WriteLine();
        }
    }

    private static async Task PromptAndRunAsync(
        string prompt,
        Func<string, Task<int>> command)
    {
        Console.Write($"{prompt}: ");
        var id = Console.ReadLine()?.Trim();

        if (string.IsNullOrEmpty(id))
        {
            Console.WriteLine("No account id entered, cancelled.");
            return;
        }

        await command(id).ConfigureAwait(false);
    }

    private static void StartStopServer()
    {
        if (ServerRunning)
        {
            Console.WriteLine("Stratum server is running.\n");
            Console.WriteLine("To stop gracefully, press Ctrl+C in each "
                + "server's own window.");
            Console.Write("Force-stop both instead? "
                + "Buffered writes may be lost (y/n)? ");

            var answer = Console.ReadLine()?.Trim();

            if (string.Equals(answer, "y", StringComparison.OrdinalIgnoreCase))
            {
                StopAll();
            }
            else
            {
                Console.WriteLine("Cancelled; servers left running.");
            }

            return;
        }

        switch (LoginServerProcess.Start())
        {
            case StartResult.Started:
                Console.WriteLine("LoginServer started in its own window.");
                break;
            case StartResult.AlreadyRunning:
                Console.WriteLine("LoginServer is already running.");
                break;
            case StartResult.ExecutableNotFound:
                Console.WriteLine("Could not find LoginServer.exe in any "
                    + "known location. Build the LoginServer project first.");
                break;
            case StartResult.Failed:
                Console.WriteLine("Failed to launch LoginServer. See server "
                    + "log for details.");
                break;
        }

        switch (SentinelProcess.Start())
        {
            case StartResult.Started:
                Console.WriteLine("Sentinel started in its own window.");
                break;
            case StartResult.AlreadyRunning:
                Console.WriteLine("Sentinel is already running.");
                break;
            case StartResult.ExecutableNotFound:
                Console.WriteLine("Could not find Sentinel.exe in any "
                    + "known location. Build the Sentinel project first.");
                break;
            case StartResult.Failed:
                Console.WriteLine("Failed to launch Sentinel. See server "
                    + "log for details.");
                break;
        }
    }

    private static void StopAll()
    {
        if (LoginServerProcess.IsRunning)
        {
            LoginServerProcess.ForceStop();
            Console.WriteLine("LoginServer force-stopped.");
        }

        if (SentinelProcess.IsRunning)
        {
            SentinelProcess.ForceStop();
            Console.WriteLine("Sentinel force-stopped.");
        }
    }

    private static void CheckResources()
    {
        Console.WriteLine("Not yet implemented.");
    }

    private static void PrintMenu()
    {
        Console.WriteLine("Welcome to Stratum Core");
        Console.WriteLine("Please select an option.");
        Console.WriteLine();

        if (!ServerRunning)
        {
            Console.WriteLine("  1) Start Stratum Server");
        }
        else
        {
            Console.WriteLine("  1) Stop Stratum Server");
        }

        Console.WriteLine("  2) Check Resources");
        Console.WriteLine("  3) Create Account");
        Console.WriteLine("  4) Reset Account");
        Console.WriteLine("  5) Delete Account");
        Console.WriteLine("  6) List Accounts");
        Console.WriteLine("  0) Quit");
        Console.Write("> ");
    }

    /// <summary>
    /// Checks every loaded character's species against the loaded species
    /// definitions, once at startup, after both stores have booted.
    /// </summary>
    /// <remarks>
    /// Runs here for the same reason <see cref="ReconcileLinks"/> does: it
    /// depends on two stores being loaded, which neither can guarantee from
    /// inside its own boot. A character naming a species with no definition
    /// on disk is a content gap the admin should see at boot, not a crash
    /// waiting for the first system that reads the definition.
    /// </remarks>
    private static void ResolveCharacterSpecies()
    {
        var species = SpeciesStore.Instance;
        var characters = CharacterStore.Instance;

        int resolved = 0;
        int unresolved = 0;

        foreach (var name in characters.ListNames())
        {
            if (!characters.TryGet(name, out var record))
                continue;

            if (species.TryGet(record.Species, out var definition))
            {
                resolved++;
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                    $"Character '{name}' species resolved: "
                        + $"{record.Species} -> '{definition.Name}', "
                        + $"{definition.Limbs.Count} limbs."));
            }
            else
            {
                unresolved++;
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Character '{name}' has species "
                        + $"'{record.Species}' but no definition is "
                        + "loaded for it."));
            }
        }

        Console.WriteLine(
            $"Species resolve: {resolved} resolved, "
                + $"{unresolved} unresolved of "
                + $"{species.Count} definition(s) loaded.");
    }
}
/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */