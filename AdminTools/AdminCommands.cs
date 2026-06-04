/*
 * (AdminCommands.cs)
 *------------------------------------------------------------
 * Created - 5/31/2026 9:50:22 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Accounts;
using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;

namespace SystemTools.Accounts.AdminTools;

/// <summary>
/// Provides administrative commands for managing user accounts, including creating new
/// accounts, resetting passwords, and deleting accounts.
/// </summary>
public static class AdminCommands
{
    /// <summary>
    /// Create a new account with the specified ID. This will prompt for a password
    /// </summary>
    /// <param name="accountId">The ID of the account to create.</param>
    /// <returns>A task representing the asynchronous operation. The task result 
    /// is 0 if successful, 1 otherwise.</returns>
    public static async Task<int> CreateAccountAsync(string accountId)
    {
        if (!ConsoleInput.TryValidateId(accountId, out var error))
        {
            Console.WriteLine(error);
            return 1;
        }

        if(AccountStore.Instance.TryGet(accountId, out _))
        {
            Console.WriteLine($"Account '{accountId}' already exists.");
            return 1;
        }

        var password = ConsoleInput.ReadPasswordWithConfirm();

        if (password is null)
            return 1;

        try
        {
            var hash = PasswordHasher.Hash(password);
            var keys = Ed25519KeyGenerator.Generate();
            var now = DateTime.UtcNow;

            var record = new AccountRecord
            {
                Id = accountId,
                PublicKey = Convert.ToBase64String(keys.PublicKey),
                PasswordHash = hash,
                IssuedAt = now,
                CreatedAt = now
            };

            AccountStore.Instance.Add(record);
            await DiskManager.Instance.FlushAsync().ConfigureAwait(false);

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Account '{accountId}' created successfully."));

            Console.WriteLine($"Account '{accountId}' created successfully.");

            PrintIssuedKey(accountId, keys.PrivateSeed);
            return 0;
        }
        catch(Exception ex)
        {
            Console.WriteLine(
                $"Failed to create account '{accountId}': {ex.Message}\n{ex.StackTrace}");
            return 1;
        }
    }

    /// <summary>
    /// List all existing accounts in the account store. This will display the account 
    /// IDs and their creation timestamps.
    /// </summary>
    /// <returns>The number of accounts listed.</returns>
    public static int ListAccounts()
    {
        var ids = AccountStore.Instance.ListIds();

        if(ids.Count == 0)
        {
            Console.WriteLine("No accounts found.");
            return 0;
        }

        Console.WriteLine($"{ids.Count} account(s) found.");

        foreach(var id in ids)
        {
            if(AccountStore.Instance.TryGet(id, out var record)) 
            {
                var created = DateTime.SpecifyKind(record.CreatedAt, 
                    DateTimeKind.Utc);

                Console.WriteLine($"  {id}  (created {created:u}");
            }
            else
            {
                Console.WriteLine($"  {id}");
            }
        }

        return 0;
    }

    /// <summary>
    /// Delete an existing account by ID. This will remove the account record from
    /// the account store and flush the changes to disk.
    /// </summary>
    /// <param name="accountId">The ID of the account to delete.</param>
    /// <returns>A task representing the asynchronous operation. The task result 
    /// is 0 if successful, 1 otherwise.</returns>
    public static async Task<int> DeleteAccountAsync(string accountId)
    {
        if(!ConsoleInput.TryValidateId(accountId, out var error))
        {
            Console.WriteLine(error);
            return 1;
        }

        try
        {
            AccountStore.Instance.Remove(accountId);
            await DiskManager.Instance.FlushAsync().ConfigureAwait(false);

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Account '{accountId}' deleted successfully."));

            Console.WriteLine($"Account '{accountId}' deleted successfully.");

            return 0;
        }
        catch(Exception ex)
        {
            Console.WriteLine(
                $"Failed to delete account '{accountId}': {ex.Message}\n" +
                $"{ex.StackTrace}");
            return 1;
        }
    }

    /// <summary>
    /// Reset the password for an existing account. This will prompt for a 
    /// new password and update the account record.
    /// </summary>
    /// <param name="accountId">The ID of the account to reset the password for.</param>
    /// <returns>A task representing the asynchronous operation. The task result 
    /// is 0 if successful, 1 otherwise.</returns>
    public static async Task<int> ResetPasswordAsync(string accountId)
    {
        if(!ConsoleInput.TryValidateId(accountId, out var error))
        {
            Console.WriteLine(error);
            return 1;
        }

        if(!AccountStore.Instance.TryGet(accountId, out var existing))
        {
            Console.WriteLine($"Account '{accountId}' does not exist.");
            return 1;
        }

        var password = ConsoleInput.ReadPasswordWithConfirm();
        
        if (password is null)
            return 1;

        try
        {
            var updated = new AccountRecord
            {
                Id = existing.Id,
                PublicKey = existing.PublicKey,
                PasswordHash = PasswordHasher.Hash(password),
                IssuedAt = existing.IssuedAt,
                CreatedAt = existing.CreatedAt
            };

            AccountStore.Instance.Update(updated);
            await DiskManager.Instance.FlushAsync().ConfigureAwait(false);

            Scribe.Pump(new ScribeMessage(ScribeSeverity.Info,
                $"Password reset for account '{accountId}'."));
            Console.WriteLine($"Password reset for account '{accountId}'.");

            return 0;
        }
        catch(Exception ex)
        {
            Console.WriteLine(
                $"Failed to reset password for '{accountId}': {ex.Message}\n" +
                $"{ex.StackTrace}");
            return 1;
        }
    }

    private static void PrintIssuedKey(string accountId, byte[] privateSeed)
    {
        Console.WriteLine();
        Console.WriteLine($"Account '{accountId}' created.\n" +
            $"Private key (base64) - shown once, deliver out-of-band:");
        
        Console.WriteLine(Convert.ToBase64String(privateSeed));
        
        Console.WriteLine();
    }
}



/*
 *------------------------------------------------------------
 * (AdminCommands.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */