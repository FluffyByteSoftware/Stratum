/*
 * (DiskEntry.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 4:30:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools.Storage;

/// <summary>
/// An internal cache entry representing a dirty file awaiting flush to disk. Carries the relative path,
/// the byte payload, and the timestamp of the most recent write.
/// </summary>
/// <remarks>Constructed and owned internally by <see cref="DiskManager"/>. Callers never interact with
/// this type directly; they use the public read/write methods on <see cref="DiskManager"/>.</remarks>
internal readonly struct DiskEntry(string path, byte[] data)
{
    /// <summary>
    /// The relative path of the file, rooted at the DiskManager's configured root path.
    /// </summary>
    public string Path { get; } = path;

    /// <summary>
    /// The byte payload to be written to disk on the next flush.
    /// </summary>
    public byte[] Data { get; } = data;

    /// <summary>
    /// The UTC timestamp of the most recent write that produced this entry.
    /// </summary>
    public DateTime LastWriteUtc { get; } = DateTime.UtcNow;
}

/*
 *------------------------------------------------------------
 * (DiskEntry.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */