/*
 * (AccountCharacterReconciler.cs)
 *------------------------------------------------------------
 * Created - 6/26/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Accounts;
using SystemTools.Logger;

namespace SystemTools.Characters;

/// <summary>
/// Reconciles the bidirectional account-character link at startup, after both
/// <see cref="AccountStore"/> and <see cref="CharacterStore"/> have completed their
/// boot scans. Heals the one anomaly that is safe to repair automatically — an orphan
/// character file whose account has no back-reference — and warns on everything else,
/// touching nothing it cannot fix without risk.
/// </summary>
/// <remarks>
/// <para>
/// The link is bidirectional on disk: <see cref="AccountRecord.CharacterName"/> points
/// at the character, and <see cref="CharacterRecord.AccountId"/> points back at the
/// account. Either side alone is enough to re-derive the other, which is what makes the
/// orphan-heal possible. This reconciler is its own object run by the process — never
/// invoked inside a store's <c>Initialize</c>, which would be the wrong dependency
/// direction (a single store cannot reach across to another during its own boot).
/// </para>
/// <para>
/// It operates purely on the records the two stores loaded into memory; it never stats
/// the filesystem itself. The stores' boot scans already skip-and-warn on any file they
/// could not read or parse, so a corrupt file is simply absent from memory here — and
/// crucially, <b>indistinguishable from a genuinely missing file</b> at this layer. That
/// indistinguishability is the whole reason the destructive paths are Warn-and-leave: a
/// dangling account reference might point at a file that is merely corrupt and fully
/// recoverable, so clearing the reference (which would route the player into character
/// creation and overwrite that file on the next <c>Add</c>) is exactly the data-loss the
/// project's "a parse error is never 'missing'" rule forbids. The decision of how to
/// route a player whose character will not load belongs to the login-to-world branch,
/// which has the player in front of it, not to a silent startup mutation.
/// </para>
/// <para>
/// The single automatic write is purely additive: re-stamping an account's empty
/// <see cref="AccountRecord.CharacterName"/> from a character that already names it as
/// owner. The reconciler never deletes a character, never severs a live link, and never
/// fabricates an account for an ownerless character.
/// </para>
/// </remarks>
public static class AccountCharacterReconciler
{
    /// <summary>
    /// Runs both reconciliation passes and returns a summary of the outcome.
    /// </summary>
    /// <returns>A <see cref="ReconcileReport"/> tallying healthy links, automatic heals,
    /// and anomalies left for operator review.</returns>
    /// <remarks>
    /// Two stateless passes. Pass one walks characters and is the only pass that writes —
    /// always to the <i>account</i> side, never to the character it is iterating, so the
    /// character enumeration is never mutated underneath itself. Pass two walks accounts
    /// and re-derives link health from scratch; because pass one's heals are already
    /// committed and visible to pass two's reads, an account healed in pass one reads as
    /// healthy in pass two and is not double-counted or double-warned.
    /// </remarks>
    public static ReconcileReport Run()
    {
        var accounts = AccountStore.Instance;
        var characters = CharacterStore.Instance;

        var healthy = 0;
        var healed = 0;
        var anomalies = 0;

        // Pass 1 — character-driven. Each loaded character names an owning account;
        // confirm the account agrees, and heal the one safe disagreement.
        foreach (var characterName in characters.ListNames())
        {
            if (!characters.TryGet(characterName, out var character))
                continue;

            if (string.IsNullOrEmpty(character.AccountId))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Character '{characterName}' has no AccountId; "
                        + "ownerless character left untouched."));
                anomalies++;
                continue;
            }

            if (!accounts.TryGet(character.AccountId, out var account))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Character '{characterName}' names account "
                        + $"'{character.AccountId}', which is not loaded; "
                        + "left untouched."));
                anomalies++;
                continue;
            }

            if (string.Equals(
                account.CharacterName,
                characterName,
                StringComparison.OrdinalIgnoreCase))
            {
                healthy++;
                continue;
            }

            if (string.IsNullOrEmpty(account.CharacterName))
            {
                // The only automatic write: the character claims this account and the
                // account's reference is empty, so re-stamp it. Purely additive — it
                // restores a link both sides already imply, creating nothing new.
                try
                {
                    accounts.Update(account.WithCharacterName(characterName));
                    Scribe.Pump(new ScribeMessage(
                        ScribeSeverity.Info,
                        $"Healed orphan character '{characterName}': "
                            + $"re-stamped account '{account.Id}' "
                            + "back-reference."));
                    healed++;
                }
                catch (Exception ex)
                {
                    // A failed heal-write is server-induced (disk fault), so it is an
                    // Error, not a Warn. The pass continues — one stuck heal must not
                    // abort reconciliation of the rest.
                    Scribe.Pump(new ScribeMessage(
                        ScribeSeverity.Error,
                        $"Failed to heal orphan character '{characterName}' "
                            + $"onto account '{account.Id}'.",
                        ex));
                    anomalies++;
                }

                continue;
            }

            // The account already names a different character. Two characters cannot be
            // reconciled onto one account automatically; this is a contradiction for the
            // operator to resolve.
            Scribe.Pump(new ScribeMessage(
                ScribeSeverity.Warn,
                $"Character '{characterName}' claims account "
                    + $"'{account.Id}', but that account names "
                    + $"'{account.CharacterName}'; left untouched."));
            anomalies++;
        }

        // Pass 2 — account-driven. Catches anomalies invisible to pass 1: accounts whose
        // reference points at a character that did not load, or whose named character
        // disagrees about ownership. Healthy and already-healed accounts re-derive as
        // healthy here and fall through silently.
        foreach (var accountId in accounts.ListIds())
        {
            if (!accounts.TryGet(accountId, out var account))
                continue;

            // Empty reference is the legitimate "no character yet" state — the
            // create-fork the login branch routes on. Not an anomaly.
            if (string.IsNullOrEmpty(account.CharacterName))
                continue;

            if (!characters.TryGet(account.CharacterName, out var character))
            {
                // Dangling reference: the named character is not in memory, which means
                // its file is missing OR merely failed to load. Warn-and-leave — see the
                // type remarks for why this must never auto-clear.
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Account '{accountId}' references character "
                        + $"'{account.CharacterName}', which is not loaded; "
                        + "reference left in place for operator review."));
                anomalies++;
                continue;
            }

            // The character loaded and agrees → already counted healthy (or healed) in
            // pass 1; do not re-count. Only the disagreement is new information here.
            if (!string.Equals(
                character.AccountId,
                accountId,
                StringComparison.OrdinalIgnoreCase))
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Account '{accountId}' references character "
                        + $"'{account.CharacterName}', but that character "
                        + $"names account '{character.AccountId}'; "
                        + "left untouched."));
                anomalies++;
            }
        }

        return new ReconcileReport(healthy, healed, anomalies);
    }
}

/// <summary>
/// A summary of one reconciliation run: how many links were already consistent, how many
/// were automatically healed, and how many anomalies were left for operator review.
/// </summary>
/// <param name="Healthy">Links that were already consistent on both sides.</param>
/// <param name="Healed">Orphan characters whose account back-reference was re-stamped.</param>
/// <param name="Anomalies">Inconsistencies warned about and left untouched, plus any
/// heal that failed to write.</param>
/// <remarks>
/// Lets the caller emit a single startup summary line — clean versus needs-review —
/// while the per-case detail goes to <see cref="Scribe"/> during the run.
/// </remarks>
public readonly record struct ReconcileReport(
    int Healthy,
    int Healed,
    int Anomalies);

/*
 *------------------------------------------------------------
 * (AccountCharacterReconciler.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */