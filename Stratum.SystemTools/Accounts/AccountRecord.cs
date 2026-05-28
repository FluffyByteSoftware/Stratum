/*
 * (AccountRecord.cs)
 *------------------------------------------------------------
 * Created - 5/21/2026 3:08:03 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text.Json.Serialization;

namespace Stratum.SystemTools.Accounts;

public sealed class AccountRecord
{
    [JsonPropertyName("id")]
    public string Id { get; init; } = "";

    [JsonPropertyName("publicKey")]
    public string PublicKey { get; init; } = "";

    [JsonPropertyName("passwordHash")]
    public string PasswordHash { get; init; } = "";

    [JsonPropertyName("issuedAt")]
    public DateTime IssuedAt { get; init; }

    [JsonPropertyName("createdAt")]
    public DateTime CreatedAt { get; init; }
}



/*
 *------------------------------------------------------------
 * (AccountRecord.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */