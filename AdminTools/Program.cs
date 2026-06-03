/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 6/2/2026 9:25:55 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Accounts;
using SystemTools.Logger;
using SystemTools.Storage;

namespace AdminTools;

/// <summary>
/// The entry point for the Stratum administrative CLI. 
/// Parses the command verb, initializes the storage and account subsystems.
/// Dispatches to the matching command, and tears everything down in 
/// reverse order on exit.
/// </summary>
internal static class Program
{
    private const string DataRoot = "./data";

    private static async Task<int> Main(string[] args) 
    {
        if(args.Length == 0)
        {
            PrintUsage();
            return 1;
        }

        DiskManager.Initialize(DataRoot);
        AccountStore.Initialize();

        int exitCode;

        try
        {
            exitCode = await DispatchAsync(args).ConfigureAwait(false);
        }
        catch(Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Unhandle exception in AdminTools.", ex));

            Console.WriteLine($"Error processing command.  Check core console " +
                $"for detailed log.");

            exitCode = 1;
        }
        finally
        {
            if (AccountStore.IsRunning)
            {
                await AccountStore.Instance.ShutdownAsync().ConfigureAwait(false);
            }

            await Scribe.ShutdownAsync().ConfigureAwait(false);
            if (DiskManager.IsRunning)
                await DiskManager.Instance.ShutdownAsync().ConfigureAwait(false);
        }

        return exitCode;
    }

    private static async Task<int> DispatchAsync(string[] args)
    {
        var verb = args[0];
        switch (verb)
        {
            case "create-account":
                return await RequireIdAsync(args, verb,
                    AdminCommands.CreateAccountAsync).ConfigureAwait(false);
            case "list-accounts":
                return AdminCommands.ListAccounts();
            case "delete-account":
                return await RequireIdAsync(args, verb,
                    AdminCommands.DeleteAccountAsync).ConfigureAwait(false);
            case "reset-password":
                return await RequireIdAsync(args, verb,
                    AdminCommands.ResetPasswordAsync).ConfigureAwait(false);
            default:
                Console.WriteLine($"Unrecognized command: '{verb}'.");
                PrintUsage();
                return 1;
        }
    }

    private static async Task<int> RequireIdAsync(
        string[] args,
        string verb,
        Func<string, Task<int>> command)
    {
        if(args.Length < 2)
        {
            Console.WriteLine($"'{verb}' requires an account id.");
            Console.WriteLine($"Usage: {verb} <account-id>");
            return 1;
        }

        return await command(args[1]).ConfigureAwait(false);
    }

    private static void PrintUsage()
    {
        Console.WriteLine("Stratum AdminTools");
        Console.WriteLine($"Usage: ");
        Console.WriteLine($"  create-account <account-id>");
        Console.WriteLine($"  list-accounts");
        Console.WriteLine($"  delete-account <account-id>");
        Console.WriteLine($"  reset-password <account-id>");
    }



}



/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */