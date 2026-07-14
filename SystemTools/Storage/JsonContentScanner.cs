/*
 * (JsonContentScanner.cs)
 *------------------------------------------------------------
 * Created - 7/14/2026 3:06:57 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text.Json;
using SystemTools.Logger;

namespace SystemTools.Storage;

/// <summary>
/// The shared loop body for boot-scanning hand-authored JSON content:
/// read each file through <see cref="DiskManager"/>, deserialize it,
/// hand the result to a caller-supplied acceptance check, and
/// skip-and-warn on anything that fails along the way. Never writes,
/// repairs, or deletes — a bad content file is an operator problem to
/// fix on disk.
/// </summary>
/// <remarks>
/// Extracted once the zone manifest scan became the third scanner of
/// this shape (after <c>CharacterStore</c> and <c>SpeciesStore</c>).
/// Deliberately owns only the per-file loop: enumeration stays with the
/// caller because the two live walk shapes differ (files in one
/// directory versus one fixed-name file per child directory), and two
/// examples are not enough to justify a walk-mode knob here. The
/// acceptance callback returns a rejection reason rather than a bool so
/// validation policy lives with the caller while the skip-and-warn
/// formatting stays in one place.
/// </remarks>
public static class JsonContentScanner
{
    /// <summary>
    /// Loads every file in <paramref name="relativePaths"/>, passing
    /// each successfully parsed value to <paramref name="accept"/>.
    /// </summary>
    /// <typeparam name="T">The content definition type each file
    /// deserializes to.</typeparam>
    /// <param name="relativePaths">Paths relative to the
    /// <see cref="DiskManager"/> root, one per content file, in the
    /// order the caller enumerated them.</param>
    /// <param name="accept">Validation-and-storage callback. Receives
    /// the parsed value and the file name; returns <c>null</c> to
    /// accept (the caller has stored it and logged its own success
    /// line) or a human-readable reason to skip-and-warn. The callback
    /// owns duplicate detection, since only the caller's store can see
    /// a collision.</param>
    /// <returns>The number of files accepted.</returns>
    /// <remarks>
    /// A file that fails to read, fails to parse, or parses to
    /// <c>null</c> (a literal JSON <c>null</c> body) is skipped with a
    /// warning; the scan always continues to the next file. The caller
    /// logs its own summary line after this returns — summary phrasing
    /// belongs to the store, not the loop.
    /// </remarks>
    public static int Load<T>(
        IEnumerable<string> relativePaths,
        Func<T, string, string?> accept)
    {
        var disk = DiskManager.Instance;
        var accepted = 0;

        foreach (var relativePath in relativePaths)
        {
            var fileName = Path.GetFileName(relativePath);

            T? value;
            try
            {
                var json = disk.ReadTextFile(relativePath);
                value = JsonSerializer.Deserialize<T>(
                    json,
                    JsonConfigurator.ContentIndented);
            }
            catch (Exception ex)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Failed to load content file '{relativePath}'.",
                    ex));
                continue;
            }

            if (value is null)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Content file '{relativePath}' parsed to null; "
                        + "skipping."));
                continue;
            }

            var reason = accept(value, fileName);
            if (reason is not null)
            {
                Scribe.Pump(new ScribeMessage(
                    ScribeSeverity.Warn,
                    $"Skipping '{relativePath}': {reason}"));
                continue;
            }

            accepted++;
        }

        return accepted;
    }
}

/*
 *------------------------------------------------------------
 * (JsonContentScanner.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */