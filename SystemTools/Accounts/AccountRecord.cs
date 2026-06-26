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
/// a new instance. <see cref="WithCharacterName"/> exists so the one such copy a
/// caller needs today lives here, beside the field declarations, rather than being
/// hand-rolled at the call site — see its remarks.
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
}

/*
 *------------------------------------------------------------
 * (AccountRecord.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */