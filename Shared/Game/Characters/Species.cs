/*
 * (Species.cs)
 *------------------------------------------------------------
 * Created - 6/16/2026 12:01:39 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Game.Characters
{
    /// <summary>
    /// Defines the various species available for character in the game.
    /// Each species is hard coded with a unique identifier for use in
    /// creation and by NPCs.
    /// </summary>
    public enum Species
    {
        /// <summary>
        /// Default used for uninitialized characters
        /// </summary>
        None = 0,

        /// <summary>
        /// The human species, the most common and versatile species in 
        /// the game world.
        /// </summary>
        Human = 1,
        
        /// <summary>
        /// Undead represent the most common enemy type.
        /// Reanimated corpses, ghosts, ghouls, oh my.
        /// </summary>
        Undead = 2,
    }
}

/*
 *------------------------------------------------------------
 * (Species.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */