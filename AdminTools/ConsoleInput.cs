/*
 * (ConsoleInput.cs)
 *------------------------------------------------------------
 * Created - 5/31/2026 7:06:28 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text;
using System.Text.RegularExpressions;

namespace AdminTools;

/// <summary>
/// Provides utility methods for reading and validating console input, 
/// specifically for account IDs and passwords.
/// </summary>
public static partial class ConsoleInput
{
    private const int MinIdLength = 3;
    private const int MaxIdLength = 24;

    [GeneratedRegex("^[a-z]+$")]
    private static partial Regex IdCharsetPattern();

    /// <summary>
    /// Validates an account ID string against defined rules: non-empty, 
    /// length between 3 and 24 characters, and only lowercase letters.
    /// </summary>
    /// <param name="accountId">The account ID to validate.</param>
    /// <param name="error">The error message if validation fails.</param>
    /// <returns>True if the account ID is valid; otherwise, false.</returns>
    public static bool TryValidateId(string accountId, out string error)
    {
        if (string.IsNullOrEmpty(accountId))
        {
            error = "Account id must not be empty.";
            return false;
        }

        if(accountId.Length < MinIdLength || accountId.Length > MaxIdLength)
        {
            error = $"Account id length must be {MinIdLength}-{MaxIdLength}.";
            return false;
        }

        if(!IdCharsetPattern().IsMatch(accountId))
        {
            error = "Account id must only contain lowercase letters.";
            return false;
        }

        error = "";
        return true;
    }

    /// <summary>
    /// Reads a password from the console with confirmation, ensuring that the password is 
    /// not empty and that the confirmation matches.
    /// </summary>
    /// <returns>The confirmed password, or null if the input was invalid.</returns>
    public static string? ReadPasswordWithConfirm()
    {
        var first = ReadHidden("Password: ");

        if (first.Length == 0)
        {
            Console.WriteLine("Password must not be empty.");
            return null;
        }

        var second = ReadHidden("Confirm password: ");

        if(!string.Equals(first, second, StringComparison.Ordinal))
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

            if(key.Key == ConsoleKey.Enter)
            {
                Console.WriteLine();
                break;
            }

            if(key.Key == ConsoleKey.Backspace)
            {
                if(builder.Length > 0)
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