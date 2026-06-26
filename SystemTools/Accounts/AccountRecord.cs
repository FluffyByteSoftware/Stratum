/*
 * (AccountRecord.cs)
 *------------------------------------------------------------
 * Created - 5/21/2026 3:08:03 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text.Json.Serialization;

namespace SystemTools.Accounts;

/// <summary>
/// The persisted, immutable record for a single account: its identity, auth
/// material, creation timestamp, and the back-reference to its owned character.
/// </summary>
/// <remarks>
/// Every property is <c>init</c>-only on purpose. This is a load-bearing auth type,
/// verified end to end, and it is intentionally a <see langword="class"/> rather than
/// a <see langword="record"/>: <c>AccountStore.Update</c> relies on reference equality
/// in its <c>ConcurrentDictionary.TryUpdate</c> optimistic-concurrency check, and a
/// record's value equality would silently change that semantic. Mutating an existing
/// record in place is therefore impossible by design; to change a field, copy-construct
/// a new instance. The <c>With…</c> copy-helpers below exist so every such copy lives
/// here, beside the field declarations, rather than being hand-rolled at a call site —
/// see their remarks.
/// </remarks>
public sealed class AccountRecord
{
    /// <summary>
    /// Account Id.
    /// </summary>
    [JsonPropertyName("id")]
    public string Id { get; init; } = "";

    /// <summary>
    /// The public key expected for this account.
    /// </summary>
    [JsonPropertyName("publicKey")]
    public string PublicKey { get; init; } = "";

    /// <summary>
    /// The stored password hash for accessing this account.
    /// </summary>
    [JsonPropertyName("passwordHash")]
    public string PasswordHash { get; init; } = "";

    /// <summary>
    /// The DateTime in Utc of when the account's last key pair 
    /// was generated.
    /// </summary>
    [JsonPropertyName("timeKeyIssuedLast")]
    public DateTime TimeLastKeyIssued { get; init; }

    /// <summary>
    /// The DateTime of when the account was created.
    /// </summary>
    [JsonPropertyName("createdAt")]
    public DateTime CreatedAt { get; init; }

    /// <summary>
    /// The name of the character associated with this account.
    /// On null, create new character.
    /// </summary>
    [JsonPropertyName("characterName")]
    public string CharacterName { get; init; } = "";

    /// <summary>
    /// Returns a copy of this record with <see cref="CharacterName"/> replaced and
    /// every other field carried forward unchanged.
    /// </summary>
    /// <param name="characterName">The character name to stamp onto the copy.</param>
    /// <returns>A new <see cref="AccountRecord"/> identical to this one except for
    /// <see cref="CharacterName"/>.</returns>
    /// <remarks>
    /// This is the single sanctioned way to change an account's character back-reference,
    /// produced during character creation and handed to <c>AccountStore.Update</c>.
    /// Because the type is immutable and not a record, the only alternative is a manual
    /// field-by-field copy at the call site — which drops any newly added field on the
    /// floor with no compiler error. Centralizing the copy here means a future field
    /// addition is caught at the obvious place: add the field above, then carry it in
    /// the initializer below. If this type ever grows a property, it must be added to
    /// the copy below or it will silently fail to round-trip through this helper.
    /// </remarks>
    public AccountRecord WithCharacterName(string characterName)
        => new()
        {
            Id = Id,
            PublicKey = PublicKey,
            PasswordHash = PasswordHash,
            TimeLastKeyIssued = TimeLastKeyIssued,
            CreatedAt = CreatedAt,
            CharacterName = characterName,
        };

    /// <summary>
    /// Returns a copy of this record with a freshly issued key pair recorded —
    /// <see cref="PublicKey"/> and <see cref="TimeLastKeyIssued"/> replaced — and every
    /// other field, <see cref="CharacterName"/> included, carried forward unchanged.
    /// </summary>
    /// <param name="publicKey">The Base64-encoded Ed25519 public key just issued.</param>
    /// <param name="timeLastKeyIssued">The UTC instant the new key pair was minted,
    /// from which the 3-day expiry is measured.</param>
    /// <returns>A new <see cref="AccountRecord"/> identical to this one except for its
    /// public key and key-issuance timestamp.</returns>
    /// <remarks>
    /// This exists for the password-fallback path in <c>AuthHandler.OnAuthByPassword</c>,
    /// which re-mints an Ed25519 key pair on every successful password login and persists
    /// it. That path previously hand-built a fresh <see cref="AccountRecord"/> with an
    /// object initializer that listed only the five auth/identity fields and silently
    /// omitted <see cref="CharacterName"/> — so the field fell back to its empty default
    /// and the account's character link was wiped on every password login. Because keys
    /// hard-expire after three days, password login is a routine path, not a rare one, so
    /// that drop severed the link for any returning player whose key had aged out and
    /// stranded them at the login→world fork (empty reference → "create" → name already
    /// taken on disk). Routing the rekey through this helper carries <see cref="CharacterName"/>
    /// — and every future field — forward by construction, the same field-addition safety
    /// <see cref="WithCharacterName"/> provides. If this type grows a property, add it to
    /// the copy below or it will be lost on every rekey.
    /// </remarks>
    public AccountRecord WithReissuedKey(
        string publicKey,
        DateTime timeLastKeyIssued)
        => new()
        {
            Id = Id,
            PublicKey = publicKey,
            PasswordHash = PasswordHash,
            TimeLastKeyIssued = timeLastKeyIssued,
            CreatedAt = CreatedAt,
            CharacterName = CharacterName,
        };
}

/*
 *------------------------------------------------------------
 * (AccountRecord.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */