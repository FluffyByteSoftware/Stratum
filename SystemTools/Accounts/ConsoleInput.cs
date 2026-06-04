/*
 * (ConsoleInput.cs)
 *------------------------------------------------------------
 * Created - 6/3/2026 1:00:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text;
using System.Text.RegularExpressions;

namespace SystemTools.Accounts;

/// <summary>
/// Reads and validates operator console input for account management: account-id
/// validation and confirmed, masked password entry. All members write to and read
/// from the console directly and are intended for interactive use only.
/// </summary>
public static partial class ConsoleInput
{
    private const int MinIdLength = 3;
    private const int MaxIdLength = 24;

    [GeneratedRegex("^[a-z]+$")]
    private static partial Regex IdCharsetPattern();

    /// <summary>
    /// Validates an account id against the canonical rule: non-empty, length
    /// 3-24 inclusive, and lowercase letters only.
    /// </summary>
    /// <param name="accountId">The candidate account id.</param>
    /// <param name="error">A human-readable reason on failure; empty on success.</param>
    /// <returns><see langword="true"/> if valid; otherwise <see langword="false"/>.</returns>
    public static bool TryValidateId(string accountId, out string error)
    {
        if (string.IsNullOrEmpty(accountId))
        {
            error = "Account id must not be empty.";
            return false;
        }

        if (accountId.Length < MinIdLength || accountId.Length > MaxIdLength)
        {
            error = $"Account id length must be {MinIdLength}-{MaxIdLength}.";
            return false;
        }

        if (!IdCharsetPattern().IsMatch(accountId))
        {
            error = "Account id must only contain lowercase letters.";
            return false;
        }

        error = "";
        return true;
    }

    /// <summary>
    /// Reads a password from the console twice, masking input, and confirms the two
    /// entries match and are non-empty.
    /// </summary>
    /// <returns>The confirmed password, or <see langword="null"/> if empty or mismatched.</returns>
    public static string? ReadPasswordWithConfirm()
    {
        var first = ReadHidden("Password: ");

        if (first.Length == 0)
        {
            Console.WriteLine("Password must not be empty.");
            return null;
        }

        var second = ReadHidden("Confirm password: ");

        if (!string.Equals(first, second, StringComparison.Ordinal))
        {
            Console.WriteLine("Passwords do not match.");
            return null;
        }

        return first;
    }

    private static string ReadHidden(string prompt)
    {
        Console.WriteLine(prompt);
        var builder = new StringBuilder();

        while (true)
        {
            var key = Console.ReadKey(intercept: true);

            if (key.Key == ConsoleKey.Enter)
            {
                Console.WriteLine();
                break;
            }

            if (key.Key == ConsoleKey.Backspace)
            {
                if (builder.Length > 0)
                {
                    builder.Length--;
                    Console.Write("\b \b");
                }
                continue;
            }

            if (char.IsControl(key.KeyChar))
                continue;

            builder.Append(key.KeyChar);
            Console.Write('*');
        }

        return builder.ToString();
    }
}
/*
 *------------------------------------------------------------
 * (ConsoleInput.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */