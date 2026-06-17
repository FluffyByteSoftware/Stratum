/*
 * (CharacterRecord.cs)
 *------------------------------------------------------------
 * Created - 6/15/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Shared.Game.Characters;
using Shared.Game.Characters.PlayerClasses;
using System.Text.Json.Serialization;

namespace SystemTools.Characters;

/// <summary>
/// Represents a character record with properties such as name, species, level,
/// experience, and timestamps for creation and last played. This record is used
/// for storing character information in the character store and can be serialized
/// to disk as Json.
/// </summary>
public sealed class CharacterRecord
{
    /// <summary>
    /// The name of the character. Must be between 3 and 14 characters, and consist
    /// of lowercase letters only.
    /// </summary>
    [JsonPropertyName("characterName")]
    public string CharacterName { get; init; } = "";

    /// <summary>
    /// The species of the character. This is a free-form string that can be used to
    /// customize the capitalization or even the display of a user name.
    /// Titles will go in here most likely.
    /// </summary>
    [JsonPropertyName("fullName")]
    public string FullName { get; init; } = "";

    /// <summary>
    /// The account Id this character is associated with.
    /// </summary>
    [JsonPropertyName("accountId")]
    public string AccountId { get; init; } = "";

    /// <summary>
    /// For now a free form string, will be replaced by an enum.
    /// </summary>
    [JsonPropertyName("species")]
    public Species Species { get; init; } = Species.None;
    
    /// <summary>
    /// The player class of the character.
    /// </summary>
    [JsonPropertyName("playerClass")]
    public PlayerClass Class { get; init; } = new PlayerClass("Default");

    /// <summary>
    /// The level of the character. This is a non-negative integer that represents the
    /// character's progression.
    /// </summary>
    [JsonPropertyName("level")]
    public int Level { get; init; }

    /// <summary>
    /// Represents the character's current experience.
    /// </summary>
    [JsonPropertyName("experience")]
    public long Experience { get; init; }

    /// <summary>
    /// The DateTime when this character was created in Utc.
    /// </summary>
    [JsonPropertyName("createdAt")]
    public DateTime CreatedAtUtc { get; init; }

    /// <summary>
    /// The DateTime when this character was last played in Utc.
    /// </summary>
    [JsonPropertyName("lastPlayedAt")]
    public DateTime LastPlayedUtc { get; init; }
}

/*
 *------------------------------------------------------------
 * (CharacterRecord.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */