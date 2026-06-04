/*
 * (AdminToolLog.cs)
 *------------------------------------------------------------
 * Created - 6/3/2026 12:30:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Storage;

namespace SystemTools.Logger;

/// <summary>
/// Identifies an administrative account action for audit-log tagging.
/// </summary>
public enum AdminAction
{
    /// <summary>Account creation.</summary>
    Create,

    /// <summary>Account deletion.</summary>
    Delete,

    /// <summary>Account password reset.</summary>
    Reset
}

/// <summary>
/// Writes account-management audit lines to the administrative log
/// (<see cref="LogFile.Admin"/>) via <see cref="DiskManager"/>. Records both
/// successful and failed actions so the log is a complete trail of attempts.
/// </summary>
/// <remarks>This facade only writes the durable audit record; it performs no
/// console output and routes nothing through <c>Scribe</c>. Operator-facing
/// messages and exception logging are the caller's responsibility.</remarks>
public static class AdminToolLog
{
    /// <summary>
    /// Records a successful administrative action against an account.
    /// </summary>
    /// <param name="action">The action that succeeded.</param>
    /// <param name="accountId">The id of the affected account.</param>
    public static void Success(AdminAction action, string accountId)
    {
        Write(action, accountId, "OK");
    }

    /// <summary>
    /// Records a failed or rejected administrative action against an account.
    /// </summary>
    /// <param name="action">The action that failed.</param>
    /// <param name="accountId">The id of the affected account.</param>
    /// <param name="reason">A short human-readable failure reason.</param>
    public static void Failure(AdminAction action, string accountId, string reason)
    {
        Write(action, accountId, $"FAILED ({reason})");
    }

    private static void Write(AdminAction action, string accountId, string outcome)
    {
        if (!DiskManager.IsRunning) return;

        var stamp = DateTime.Now.ToString("M/d/yyyy - h:mm tt");
        var tag = action switch
        {
            AdminAction.Create => "CREATE",
            AdminAction.Delete => "DELETE",
            AdminAction.Reset => "RESET ",
            _ => "?????"
        };

        var line = $"[{stamp}] - [{tag}] - account '{accountId}' - {outcome}";
        DiskManager.Instance.Log(LogFile.Admin, line);
    }
}
/*
 *------------------------------------------------------------
 * (AdminToolLog.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */