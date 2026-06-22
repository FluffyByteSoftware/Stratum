/*
 * (Limb.cs)
 *------------------------------------------------------------
 * Created - 6/19/2026 9:17:24 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text.Json.Serialization;

namespace Game.Living.Anatomy;

/// <summary>
/// One part of a living thing's body — an arm, an eyestalk, a torso — described
/// generically enough that any creature's body plan can be expressed as a flat
/// collection of these, from a humanoid to a floating aberration.
/// </summary>
/// <remarks>
/// Every field is free-form rather than an enum on purpose. An enum would bake in
/// a humanoid skeleton (arm/leg/hand/foot) that a gazer or other aberration does
/// not fit; authoring the part as plain data lets a content file name whatever
/// the creature actually has. The body plan is therefore content, not code: the
/// system reads these parts uniformly and never needs to know the taxonomy of
/// bodies ahead of time.
/// </remarks>
public readonly record struct Limb
{
    /// <summary>
    /// The kind of body part, as the creature's content names it — "arm",
    /// "eyestalk", "torso". This is the flavor word a struck-location message
    /// reads ("...on the left arm"); it is intentionally a free string so the
    /// vocabulary is open to any creature rather than fixed in code.
    /// </summary>
    [JsonPropertyName("name")]
    public string Name { get; init; }

    /// <summary>
    /// Which one of a same-named set this part is — "left", "right", "upper",
    /// "3" — or empty for an unpaired part such as a torso. It exists separately
    /// from <see cref="Name"/> because a body may carry many parts sharing a name
    /// (eight identical eyestalks), and the name alone cannot tell them apart; the
    /// placement is the discriminator, and composes with the name for display
    /// ("{Placement} {Name}").
    /// </summary>
    [JsonPropertyName("placement")]
    public string Placement { get; init; }

    /// <summary>
    /// How big relative to other appendages is this limb?
    /// This will effect how often it is hit.  A lower integer value
    /// means less likely to be hit, and a higher integer value means 
    /// more likely to be hit.
    /// </summary>
    [JsonPropertyName("size")]
    public int Size { get; init; }
}

/*
 *------------------------------------------------------------
 * (Limb.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */