/*
 * (CharacterCreator.cs)
 *------------------------------------------------------------
 * Created - 6/22/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Shared.Game.Characters;
using SystemTools.Accounts;
using SystemTools.Characters;

namespace SystemTools.Characters;

/// <summary>
/// Creates a character for an account: validates the requested name, confirms the
/// account exists and has no character yet, then persists a new
/// <see cref="CharacterRecord"/> and stamps the account's back-reference.
/// </summary>
/// <remarks>
/// This is plain service logic — it returns a <see cref="CharacterCreateResult"/>
/// and never touches the operator console or the admin audit log. A packet handler
/// (the player-driven create flow) is the intended caller and owns any user-facing
/// messaging. Species is not yet a creation input: there is a single playable model,
/// so the record is stamped <see cref="PlayableSpecies.Human"/> directly. When
/// creation later accepts a chosen species, that input is where a species-validation
/// result would re-enter; it is deliberately absent now rather than scaffolded
/// against a case that does not exist.
/// </remarks>
public static class CharacterCreator
{
    private const PlayableSpecies DefaultSpecies = PlayableSpecies.Human;

    private const int MinNameLength = 3;
    private const int MaxNameLength = 14;

    /// <summary>
    /// Attempts to create a character for the given account from a raw,
    /// player-supplied name.
    /// </summary>
    /// <param name="accountId">The id of the owning account. Must already exist in
    /// the <see cref="AccountStore"/>.</param>
    /// <param name="rawName">The name as the player typed it. The display form is
    /// preserved in <see cref="CharacterRecord.FullName"/>; the lowercase canonical
    /// form becomes <see cref="CharacterRecord.CharacterName"/>.</param>
    /// <param name="characterName">When this method returns <see cref="CharacterCreateResult.Created"/>,
    /// contains the lowercase canonical name of the new character; otherwise an
    /// empty string.</param>
    /// <returns>A <see cref="CharacterCreateResult"/> describing the outcome.</returns>
    /// <remarks>
    /// <para>
    /// Write ordering is character-file-first, then the account back-reference. This
    /// is deliberate: if the account update fails after the character file is written,
    /// the orphan character file is inert and self-healing — the startup reconciler
    /// re-stamps the account's <see cref="AccountRecord.CharacterName"/> from the
    /// character's <see cref="CharacterRecord.AccountId"/> on the next boot. A
    /// dangling account reference (the reverse ordering's failure) actively breaks
    /// login-to-world, so the safer half to leave behind is the character file. For
    /// that reason a post-write account failure returns <see cref="CharacterCreateResult.PersistFailed"/>
    /// and does <b>not</b> delete the character: a character file is the
    /// least-replaceable thing on disk and is never auto-removed.
    /// </para>
    /// <para>
    /// The account back-reference is stamped by copy-constructing a fresh
    /// <see cref="AccountRecord"/> rather than mutating the existing one, because
    /// <see cref="AccountRecord"/> exposes <c>init</c>-only properties (it is a
    /// load-bearing, end-to-end-verified auth type left unchanged on purpose). If
    /// <see cref="AccountRecord"/> gains a field, the copy below must be extended to
    /// carry it.
    /// </para>
    /// </remarks>
    public static CharacterCreateResult Create(
        string accountId,
        string rawName,
        out string characterName)
    {
        characterName = "";

        var trimmedName = (rawName ?? "").Trim();
        var canonicalName = trimmedName.ToLowerInvariant();

        if (!IsValidName(canonicalName))
            return CharacterCreateResult.NameInvalid;

        if (!AccountStore.Instance.TryGet(accountId, out var account))
            return CharacterCreateResult.AccountNotFound;

        if (!string.IsNullOrEmpty(account.CharacterName))
            return CharacterCreateResult.AccountAlreadyHasCharacter;

        if (CharacterStore.Instance.TryGet(canonicalName, out _))
            return CharacterCreateResult.NameTaken;

        var nowUtc = DateTime.UtcNow;

        var character = new CharacterRecord
        {
            CharacterName = canonicalName,
            FullName = trimmedName,
            AccountId = account.Id,
            Species = DefaultSpecies,
            Level = 0,
            Experience = 0L,
            CreatedAtUtc = nowUtc,
            LastPlayedUtc = nowUtc,
        };

        try
        {
            CharacterStore.Instance.Add(character);
        }
        catch
        {
            // Add rolls its own in-memory state back on persist failure, so nothing
            // is stranded; a duplicate name is already screened above, leaving a disk
            // fault as the realistic cause here.
            return CharacterCreateResult.PersistFailed;
        }

        var linkedAccount = account.WithCharacterName(canonicalName);

        try
        {
            AccountStore.Instance.Update(linkedAccount);
        }
        catch
        {
            // The character file is already written. Leave it: it is inert without a
            // back-reference and the reconciler re-links it from its AccountId on the
            // next boot. Deleting it would violate the never-auto-remove-a-character
            // rule for a fault that already has a designed recovery.
            return CharacterCreateResult.PersistFailed;
        }

        characterName = canonicalName;
        return CharacterCreateResult.Created;
    }

    /// <summary>
    /// Non-throwing name check mirroring the <see cref="CharacterStore"/> rule:
    /// 3-14 lowercase letters only.
    /// </summary>
    /// <param name="canonicalName">The already-lowercased candidate name.</param>
    /// <returns><c>true</c> if the name satisfies every rule; otherwise <c>false</c>.</returns>
    /// <remarks>
    /// Validation is duplicated here intentionally so the creator can return a clean
    /// <see cref="CharacterCreateResult.NameInvalid"/> instead of catching the
    /// <see cref="ArgumentException"/> that <see cref="CharacterStore.Add"/> would
    /// throw. The charset half doubles as the screen for raw input: any non-letter the
    /// player typed survives lowercasing and fails the check, so a separate
    /// "illegal characters" pass is unnecessary.
    /// </remarks>
    private static bool IsValidName(string canonicalName)
    {
        if (string.IsNullOrEmpty(canonicalName))
            return false;

        if (canonicalName.Length < MinNameLength
            || canonicalName.Length > MaxNameLength)
            return false;

        foreach (var c in canonicalName)
        {
            if (c is < 'a' or > 'z')
                return false;
        }

        return true;
    }
}

/*
 *------------------------------------------------------------
 * (CharacterCreator.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */