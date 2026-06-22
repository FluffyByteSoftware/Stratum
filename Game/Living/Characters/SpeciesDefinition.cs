/*
 * (SpeciesDefinition.cs)
 *------------------------------------------------------------
 * Created - 6/17/2026 9:36:37 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Game.Living.Anatomy;
using Shared.Game.Characters;
using System.Text.Json.Serialization;

namespace Game.Living.Characters;

/// <summary>
/// Defines a species, which is a template for creating living entities.
/// A species defines the limbs that make up a living entity, 
/// and the weight of each limb in terms of size.
/// </summary>
public readonly record struct SpeciesDefinition
{
    /// <summary>
    /// The name of the species.
    /// </summary>
    [JsonPropertyName("name")]
    public string Name { get; init; }

    /// <summary>
    /// The limbs that this species has.
    /// </summary>
    [JsonPropertyName("limbs")]
    public IReadOnlyList<Limb> Limbs { get; init; }

    /// <summary>
    /// Gets a brief, human-readable description.
    /// </summary>
    [JsonPropertyName("description")]
    public string Description { get; init; }

    /// <summary>
    /// Lists the integer (enum) of the playable species type.
    /// </summary>
    [JsonPropertyName("id")]
    public PlayableSpecies Id { get; init; }
}
/*
 *------------------------------------------------------------
 * (SpeciesDefinition.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */