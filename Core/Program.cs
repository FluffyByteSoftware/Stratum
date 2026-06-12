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
    private const string DataRoot = "./data";

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
        if (LoginServerProcess.IsRunning)
        {
            Console.WriteLine("LoginServer is running.\n");
            Console.WriteLine("To stop it gracefully, press Ctrl+C " +
                "in its own window.");
            Console.Write("Force-stop instead? Buffered writes may be lost (y/n)? ");

            var answer = Console.ReadLine()?.Trim();

            if(string.Equals(answer, "y", StringComparison.OrdinalIgnoreCase))
            {
                LoginServerProcess.ForceStop();
                Console.WriteLine("LoginServer force-stopped");
            }
            else
            {
                Console.WriteLine("Cancelled; LoginServer left running.");
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
                Console.WriteLine("Could not find LoginServer.exe in any known location."
                    + "Build the LoginServer project first.");
                break;
            case StartResult.Failed:
                Console.WriteLine($"Failed to launch LoginServer. See server log for " +
                    $"details.");
                break;
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
        Console.WriteLine("  1) Start/Stop Stratum Server");
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