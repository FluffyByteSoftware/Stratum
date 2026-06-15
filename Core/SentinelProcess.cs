/*
 * (SentinelProcess.cs)
 *------------------------------------------------------------
 * Created - 6/13/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Diagnostics;
using SystemTools.Logger;

namespace Core;

/// <summary>
/// Owns the Sentinel child process for the Core session. The child runs in
/// its own console window (UseShellExecute), so its Scribe/Console output
/// stays out of Core's menu and its own Ctrl+C handler performs the graceful
/// flush + DiskManager shutdown. Core's only programmatic lever is ForceStop
/// (Kill), which is deliberately lossy and is not a graceful stop.
/// </summary>
/// <remarks>
/// Sentinel and LoginServer are siblings under Core, not parent and child of
/// each other: each survives the other's restart. <see cref="StartResult"/>
/// is shared with <see cref="LoginServerProcess"/>.
/// </remarks>
internal static class SentinelProcess
{
    private const string ExeName = "Sentinel.exe";
    private const string SentinelDir = "Sentinel";

    private static readonly string[] BuildConfigurations = ["Debug", "Release"];

    private static Process? _process;

    /// <summary>
    /// Check to see if the Sentinel process is currently running.
    /// </summary>
    public static bool IsRunning
    {
        get
        {
            Process? p = _process;

            if (p is null)
                return false;
            try
            {
                return !p.HasExited;
            }
            catch (InvalidOperationException)
            {
                return false;
            }
            catch
            {
                return false;
            }
        }
    }

    /// <summary>
    /// Attempts to start the Sentinel process.
    /// </summary>
    /// <returns>The result (either process started or failed).</returns>
    public static StartResult Start()
    {
        if (IsRunning)
        {
            return StartResult.AlreadyRunning;
        }

        // Reap a stale handle from a child that already exited.
        _process?.Dispose();
        _process = null;

        string? exe = ResolveExecutable();

        if (exe is null)
            return StartResult.ExecutableNotFound;

        var startInfo = new ProcessStartInfo
        {
            FileName = exe,
            UseShellExecute = true,
            WorkingDirectory = Path.GetDirectoryName(exe)!,
        };

        try
        {
            _process = Process.Start(startInfo);
            return _process is null ? StartResult.Failed : StartResult.Started;
        }
        catch (Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Failed to launch Sentinel from '{exe}'.", ex));

            return StartResult.Failed;
        }
    }

    /// <summary>
    /// Forces the process to terminate. This is not a graceful shutdown.
    /// </summary>
    public static void ForceStop()
    {
        if (_process is null)
            return;

        try
        {
            if (!_process.HasExited)
                _process.Kill();
        }
        catch
        {
            Console.WriteLine("Force kill called on this process.");
            // Nothing to implement
        }
        finally
        {
            _process.Dispose();
            _process = null;
        }
    }

    private static string? ResolveExecutable()
    {
        var candidates = new List<string>
        {
            Path.Combine(AppContext.BaseDirectory, ExeName)
        };

        string baseDir =
            AppContext.BaseDirectory.TrimEnd(Path.DirectorySeparatorChar);

        string tfm = Path.GetFileName(baseDir);

        string? root = FindProjectRoot(baseDir);

        if (root is not null)
        {
            foreach (string config in BuildConfigurations)
            {
                candidates.Add(Path.Combine(
                    root, SentinelDir, "bin", config, tfm, ExeName));
            }
        }

        return candidates.Where(File.Exists)
            .OrderByDescending(File.GetLastWriteTimeUtc)
            .FirstOrDefault();
    }

    private static string? FindProjectRoot(string start)
    {
        var dir = new DirectoryInfo(start);

        while (dir is not null)
        {
            if (Directory.Exists(Path.Combine(dir.FullName, SentinelDir)))
                return dir.FullName;

            dir = dir.Parent;
        }

        return null;
    }
}

/*
 *------------------------------------------------------------
 * (SentinelProcess.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */