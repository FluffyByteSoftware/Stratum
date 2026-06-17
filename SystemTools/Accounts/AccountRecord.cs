/*
 * (AccountRecord.cs)
 *------------------------------------------------------------
 * Created - 5/21/2026 3:08:03 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text.Json.Serialization;

namespace SystemTools.Accounts;

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

}



/*
 *------------------------------------------------------------
 * (AccountRecord.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */