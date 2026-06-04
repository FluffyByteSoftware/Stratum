/*
 * (LogFile.cs)
 *------------------------------------------------------------
 * Created - 6/3/2026 11:08:25 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools.Storage;

/// <summary>
/// Identifies a destination log file managed by <see cref="DiskManager"/>.
/// Each member maps to its own daily-rolling file, 
/// </summary>
public enum LogFile
{
    /// <summary>
    /// General server runtime log.
    /// </summary>
    Server,
    /// <summary>
    /// Administrative audit log for account-manage actions.
    /// </summary>
    Admin,
    /// <summary>
    /// Simulation log for non-network game logic output.
    /// </summary>
    Simulation
}



/*
 *------------------------------------------------------------
 * (LogFile.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */