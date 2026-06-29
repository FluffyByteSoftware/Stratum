/*
 * (CharacterCreateResult.cs)
 *------------------------------------------------------------
 * Created - 6/26/2026 4:24:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools.Characters;

/// <summary>
/// The outcome of a <see cref="CharacterCreator.Create"/> call.
/// </summary>
public enum CharacterCreateResult
{
    /// <summary>
    /// Default / uninitialized sentinel. Never returned by a completed call; its
    /// presence signals an unset result.
    /// </summary>
    None = 0,

    /// <summary>
    /// The character was created and both the character file and the account
    /// back-reference were persisted.
    /// </summary>
    Created = 1,

    /// <summary>
    /// The requested name failed validation (empty, wrong length, or contained
    /// anything other than letters).
    /// </summary>
    NameInvalid = 2,

    /// <summary>
    /// No account exists for the supplied account id.
    /// </summary>
    AccountNotFound = 3,

    /// <summary>
    /// The account already owns a character. One character per account is a
    /// deliberate model constraint.
    /// </summary>
    AccountAlreadyHasCharacter = 4,

    /// <summary>
    /// A character with the requested canonical name already exists. Names are
    /// globally unique.
    /// </summary>
    NameTaken = 5,

    /// <summary>
    /// A disk write failed. The character file may exist without an account
    /// back-reference; the startup reconciler re-links it on the next boot.
    /// </summary>
    PersistFailed = 6,
}

/*
 *------------------------------------------------------------
 * (CharacterCreateResult.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */