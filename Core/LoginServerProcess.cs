/*
 * (LoginServerProcess.cs)
 *------------------------------------------------------------
 * Created - 6/11/2026 8:09:40 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Diagnostics;
using SystemTools.Logger;

namespace Core;

/// <summary>
/// Outcome of a Start request, so the menu can report precisely.
/// Started = Currently starting, but not yet ready to accept connections.
/// AlreadyRunning = The process is already running, and the menu should
/// not attempt to start the process again.
/// ExecutableNotFound = The executable was not found, and the menu should
/// not attempt to start the process again.
/// Failed = The process failed to start, and the menu should not attempt 
/// to start the process again.
/// </summary>
internal enum StartResult 
{
    Started,
    AlreadyRunning,
    ExecutableNotFound,
    Failed
}

/// <summary>
/// Owns the login server child process for the Core session. The child
/// runs in its own console window (UseShellExecute), so its Scribe/Console
/// output stays out of Core's menu and its own Ctrl+C handler performs the
/// graceful flush + AccountStore shutdown.  Core's only programmatic lever is 
/// ForceStop (Kill), which is deliberately lossy and is not a graceful stop.
/// </summary>
internal static class LoginServerProcess
{
    private const string ExeName = "LoginServer.exe";
    private const string LoginServerDir = "LoginServer";

    private static readonly string[] BuildConfigurations = ["Debug", "Release"];

    private static Process? _process;

    /// <summary>
    /// Check to see if the LoginServerProcess is currently running.
    /// </summary>
    public static bool IsRunning
    {
        get
        {
            Process? p = _process;

            if (p is null)
            {
                return false;
            }

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
    /// Attempts to start the LoginServerProcess.
    /// </summary>
    /// <returns>The result (either process started or failed)</returns>
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
        catch(Exception ex)
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                $"Failed to launch LoginServer from '{exe}'.", ex));

            return StartResult.Failed;
        }

    }

    /// <summary>
    /// For the process to terminate.  This is not a graceful shutdown.
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

        if(root is not null)
        {
            foreach(string config in BuildConfigurations)
            {
                candidates.Add(Path.Combine(
                    root, LoginServerDir, "bin", config, tfm, ExeName));
            }
        }

        return candidates.Where(File.Exists)
            .OrderByDescending(File.GetLastWriteTimeUtc)
            .FirstOrDefault();
    }

    private static string? FindProjectRoot(string start)
    {
        var dir = new DirectoryInfo(start);

        while(dir is not null)
        {
            if (Directory.Exists(Path.Combine(dir.FullName, LoginServerDir)))
                return dir.FullName;

            dir = dir.Parent;
        }

        return null;
    }
}



/*
 *------------------------------------------------------------
 * (LoginServerProcess.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */