/*
 * (Program.cs)
 *------------------------------------------------------------
 * Created - 6/11/2026 1:04:18 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Shared.Networking.Packets.Auth;

namespace Probe;

/// <summary>
/// Entry point and orchestrator for the Stratum auth probe. Drives the legs in
/// order and decides, from each leg's login→world outcome, how far the run
/// proceeds.
/// </summary>
/// <remarks>The legs themselves live in <see cref="AuthLegs"/> (password + key
/// auth), <see cref="CreateLeg"/> (character create), and <see cref="UdpLegs"/>
/// (UDP session + protocol version), all built on <see cref="ProbeTransport"/>.
/// This file owns only the sequencing: prompt for credentials, run legs 1-2,
/// and branch on the outcome - an Ok account already holds a token and proceeds
/// straight to the UDP legs, while a NeedsCharacter account creates a character,
/// re-authenticates to mint its token through the single verified path, then
/// proceeds. The two routes converge on one UDP call with whichever token is in
/// hand.</remarks>
internal static class Program
{
    private static async Task Main()
    {
        Console.WriteLine("Stratum auth probe");
        Console.WriteLine(
            $"Target: {ProbeTransport.ServerHost}:{ProbeTransport.ServerPort}");
        Console.WriteLine();

        Console.Write("Account id [testuser]: ");
        string accountId = ReadLineOrDefault("testuser");

        // Plain-text entry: this is a local test tool, not the real client.
        Console.Write("Password: ");
        string password = ReadLineOrDefault(string.Empty);

        try
        {
            // Leg 1 - password auth. On success the server mints a fresh
            // keypair, rewrites the account, and returns the private seed.
            // It then applies the login→world decision: a character-bearing
            // account answers Ok with a token; a character-less one answers
            // NeedsCharacter with no token, but the freshly minted seed still
            // rides so the account can key-auth on its create round-trip.
            Console.WriteLine();
            Console.WriteLine("[1] Password auth ...");

            if (await AuthLegs.PasswordAuthAsync(accountId, password)
                    .ConfigureAwait(false) is not { } pwdResponse)
                return;

            if (!AuthLegs.ReportAuthLeg(1, pwdResponse))
                return;

            if (string.IsNullOrEmpty(pwdResponse.IssuedPrivateKey))
            {
                // The password path mints and persists a key on every success,
                // for either outcome, so an empty seed here is anomalous and
                // there is nothing to key-auth with.
                Console.WriteLine(
                    "    FAIL  [1] Success returned no private key; "
                    + "cannot continue to key auth.");
                return;
            }

            byte[] seed = Convert.FromBase64String(pwdResponse.IssuedPrivateKey);
            Console.WriteLine($"    Issued seed: {seed.Length} bytes.");

            // Leg 2 - key auth using the seed leg 1 just minted. This closes
            // the auth loop independently of the character state: the key must
            // authenticate on its own, and the server re-applies the same
            // login→world decision, so this leg's outcome mirrors leg 1's for
            // the same account.
            Console.WriteLine();
            Console.WriteLine("[2] Key auth with issued seed ...");

            if (await AuthLegs.KeyAuthAsync(accountId, seed)
                    .ConfigureAwait(false) is not { } keyResponse)
                return;

            if (!AuthLegs.ReportAuthLeg(2, keyResponse))
                return;

            // Leg 5 - character create, only when the account has no character.
            // An Ok here means a character already exists and the token is in
            // hand, so create is skipped and the run proceeds straight to the
            // UDP legs - which is what makes this tool re-runnable.
            string token = keyResponse.SessionToken;

            if (keyResponse.Outcome != AuthOutcome.Ok)
            {
                Console.WriteLine();
                Console.WriteLine("[5] Character create (NeedsCharacter) ...");

                Console.Write("    Character name: ");
                string name = ReadLineOrDefault(string.Empty);

                if (string.IsNullOrEmpty(name))
                {
                    Console.WriteLine(
                        "    FAIL  [5] No name entered; cannot create.");
                    return;
                }

                if (!await CreateLeg.CreateCharacterAsync(accountId, seed, name)
                        .ConfigureAwait(false))
                    return;

                // Created closes the connection clean; the account now owns a
                // character, so a re-auth lands Ok and mints the token through
                // the single verified issuance path. Same seed - key auth never
                // re-mints, so it stays valid across the create round trip.
                Console.WriteLine();
                Console.WriteLine("[2b] Re-auth after create ...");

                if (await AuthLegs.KeyAuthAsync(accountId, seed)
                        .ConfigureAwait(false) is not { } reauthResponse)
                    return;

                if (!AuthLegs.ReportAuthLeg(2, reauthResponse))
                    return;

                if (reauthResponse.Outcome != AuthOutcome.Ok)
                {
                    Console.WriteLine(
                        "    FAIL  [2b] Expected Ok after create; got "
                        + $"{reauthResponse.Outcome}.");
                    return;
                }

                token = reauthResponse.SessionToken;
            }

            // Legs 3 + 4 - UDP auth against Sentinel using the token in hand
            // (from leg 2, or from the post-create re-auth), followed by the
            // protocol version check that follows a successful admission. Both
            // run over the same persistent LiteNetLib connection.
            Console.WriteLine();
            Console.WriteLine("[3] UDP auth with session token ...");
            Console.WriteLine("[4] Protocol version check ...");
            Console.WriteLine("[6] Keep-alive ping/pong echo ...");

            UdpLegs.UdpAuth(token);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"Probe failed: {ex.Message}");
        }
    }

    private static string ReadLineOrDefault(string fallback)
    {
        string? line = Console.ReadLine();
        return string.IsNullOrWhiteSpace(line) ? fallback : line.Trim();
    }
}

/*
 *------------------------------------------------------------
 * (Program.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */