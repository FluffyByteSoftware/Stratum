/*
 * (AccountManager.cs)
 *------------------------------------------------------------
 * Created - 6/3/2026 1:30:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Logger;
using SystemTools.Security;
using SystemTools.Storage;

namespace SystemTools.Accounts;

/// <summary>
/// Administrative account operations driven from an interactive console: create,
/// delete, reset-password, and list. Prompts via <see cref="ConsoleInput"/>,
/// persists through <see cref="AccountStore"/>, records every outcome to the
/// administrative audit log via <see cref="AdminToolLog"/>, and routes exceptions
/// to <see cref="Scribe"/>. Console output is operator-facing only.
/// </summary>
public static class AccountManager
{
    /// <summary>
    /// Creates a new account: validates the id, prompts for a confirmed password,
    /// generates an Ed25519 keypair, persists the record, and prints the private
    /// seed once for out-of-band delivery.
    /// </summary>
    /// <param name="accountId">The id of the account to create.</param>
    /// <returns>0 on success; 1 on any validation, input, or operation failure.</returns>
    public static async Task<int> CreateAccountAsync(string accountId)
    {
        if (!ConsoleInput.TryValidateId(accountId, out var error))
        {
            Console.WriteLine(error);
            AdminToolLog.Failure(AdminAction.Create, accountId, error);
            return 1;
        }

        if (AccountStore.Instance.TryGet(accountId, out _))
        {
            Console.WriteLine($"Account '{accountId}' already exists.");
            AdminToolLog.Failure(AdminAction.Create, accountId, "already exists");
            return 1;
        }

        var password = ConsoleInput.ReadPasswordWithConfirm();

        if (password is null)
        {
            AdminToolLog.Failure(AdminAction.Create, accountId,
                "password entry cancelled");
            return 1;
        }

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
                TimeLastKeyIssued = now,
                CreatedAt = now
            };

            AccountStore.Instance.Add(record);
            await DiskManager.Instance.FlushAsync().ConfigureAwait(false);

            Console.WriteLine($"Account '{accountId}' created successfully.");
            PrintIssuedKey(accountId, keys.PrivateSeed);
            AdminToolLog.Success(AdminAction.Create, accountId);
            return 0;
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Failed to create account '{accountId}'.", ex));
            Console.WriteLine("Operation failed. See server log for details.");
            AdminToolLog.Failure(AdminAction.Create, accountId, "internal error");
            return 1;
        }
    }

    /// <summary>
    /// Deletes an existing account by id.
    /// </summary>
    /// <param name="accountId">The id of the account to delete.</param>
    /// <returns>0 on success; 1 on any validation or operation failure.</returns>
    public static async Task<int> DeleteAccountAsync(string accountId)
    {
        if (!ConsoleInput.TryValidateId(accountId, out var error))
        {
            Console.WriteLine(error);
            AdminToolLog.Failure(AdminAction.Delete, accountId, error);
            return 1;
        }

        if (!AccountStore.Instance.TryGet(accountId, out _))
        {
            Console.WriteLine($"Account '{accountId}' does not exist.");
            AdminToolLog.Failure(AdminAction.Delete, accountId, "not found");
            return 1;
        }

        try
        {
            AccountStore.Instance.Remove(accountId);
            await DiskManager.Instance.FlushAsync().ConfigureAwait(false);

            Console.WriteLine($"Account '{accountId}' deleted successfully.");
            AdminToolLog.Success(AdminAction.Delete, accountId);
            return 0;
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Failed to delete account '{accountId}'.", ex));
            Console.WriteLine("Operation failed. See server log for details.");
            AdminToolLog.Failure(AdminAction.Delete, accountId, "internal error");
            return 1;
        }
    }

    /// <summary>
    /// Resets the password for an existing account, preserving its keypair and
    /// timestamps.
    /// </summary>
    /// <param name="accountId">The id of the account to reset.</param>
    /// <returns>0 on success; 1 on any validation, input, or operation failure.</returns>
    public static async Task<int> ResetPasswordAsync(string accountId)
    {
        if (!ConsoleInput.TryValidateId(accountId, out var error))
        {
            Console.WriteLine(error);
            AdminToolLog.Failure(AdminAction.Reset, accountId, error);
            return 1;
        }

        if (!AccountStore.Instance.TryGet(accountId, out var existing))
        {
            Console.WriteLine($"Account '{accountId}' does not exist.");
            AdminToolLog.Failure(AdminAction.Reset, accountId, "not found");
            return 1;
        }

        var password = ConsoleInput.ReadPasswordWithConfirm();

        if (password is null)
        {
            AdminToolLog.Failure(AdminAction.Reset, accountId,
                "password entry cancelled");
            return 1;
        }

        try
        {
            var updated = new AccountRecord
            {
                Id = existing.Id,
                PublicKey = existing.PublicKey,
                PasswordHash = PasswordHasher.Hash(password),
                TimeLastKeyIssued = existing.TimeLastKeyIssued,
                CreatedAt = existing.CreatedAt
            };

            AccountStore.Instance.Update(updated);
            await DiskManager.Instance.FlushAsync().ConfigureAwait(false);

            Console.WriteLine($"Password reset for account '{accountId}'.");
            AdminToolLog.Success(AdminAction.Reset, accountId);
            return 0;
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Failed to reset password for '{accountId}'.", ex));
            Console.WriteLine("Operation failed. See server log for details.");
            AdminToolLog.Failure(AdminAction.Reset, accountId, "internal error");
            return 1;
        }
    }

    /// <summary>
    /// Lists all existing accounts with their creation timestamps. A read-only
    /// operation; not audited.
    /// </summary>
    /// <returns>0 always, unless an unexpected error occurs.</returns>
    public static int ListAccounts()
    {
        try
        {
            var ids = AccountStore.Instance.ListIds();

            if (ids.Count == 0)
            {
                Console.WriteLine("No accounts found.");
                return 0;
            }

            Console.WriteLine($"{ids.Count} account(s) found.");

            foreach (var id in ids)
            {
                if (AccountStore.Instance.TryGet(id, out var record))
                {
                    var created = DateTime.SpecifyKind(record.CreatedAt,
                        DateTimeKind.Utc);
                    Console.WriteLine($"  {id}  (created {created:u})");
                }
                else
                {
                    Console.WriteLine($"  {id}");
                }
            }

            return 0;
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                "Failed to list accounts.", ex));
            Console.WriteLine("Operation failed. See server log for details.");
            return 1;
        }
    }

    private static void PrintIssuedKey(string accountId, byte[] privateSeed)
    {
        Console.WriteLine();
        Console.WriteLine($"Account '{accountId}' created.");
        Console.WriteLine("Private key (base64) - shown once, deliver out-of-band:");
        Console.WriteLine(Convert.ToBase64String(privateSeed));
        Console.WriteLine();
    }
}
/*
 *------------------------------------------------------------
 * (AccountManager.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */