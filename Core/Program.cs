/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 6/3/2026 7:42:47 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Accounts;
using SystemTools.Logger;
using SystemTools.Storage;

namespace Core;

internal static class Program
{
    private const string DataRoot = @"E:\Stratum\data";

    private static bool ServerRunning =>
        LoginServerProcess.IsRunning || SentinelProcess.IsRunning;

    private static async Task<int> Main()
    {
        DiskManager.Initialize(DataRoot);
        AccountStore.Initialize();

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
            if (AccountStore.IsRunning)
                await AccountStore.Instance.ShutdownAsync().ConfigureAwait(false);

            await Scribe.ShutdownAsync().ConfigureAwait(false);

            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync().ConfigureAwait(false);
        }

        return 0;
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
}
/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */