/*
 * (Scribe.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 3:45:16 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using SystemTools.Storage;
using System.Threading.Channels;

namespace SystemTools.Logger;

/// <summary>
/// A process-wide asynchronous logging facade. Callers construct a <see cref="ScribeMessage"/> and hand it
/// to <see cref="Pump(ScribeMessage)"/>; the pump drains incoming messages on a background task, writes a
/// color-coded line to the console, and forwards the formatted text to <see cref="DiskManager"/> when it
/// is running.
/// </summary>
/// <remarks>The pump is backed by a bounded channel with <see cref="BoundedChannelFullMode.DropOldest"/>
/// backpressure, so <see cref="Pump(ScribeMessage)"/> never blocks the caller. On process shutdown,
/// callers should invoke <see cref="ShutdownAsync"/> from a <c>finally</c> block <em>before</em>
/// shutting down <see cref="DiskManager"/>, so any in-flight log messages land in the log buffer
/// before the buffer is flushed and the manager stops.</remarks>
public static class Scribe
{
    private const int ChannelCapacity = 1024;

    private static readonly Channel<ScribeMessage> _channel =
        Channel.CreateBounded<ScribeMessage>(new BoundedChannelOptions(ChannelCapacity)
        {
            FullMode = BoundedChannelFullMode.DropOldest,
            SingleReader = true,
            SingleWriter = false
        });

    private static readonly CancellationTokenSource _cts = new();
    private static readonly Task _pumpTask = Task.Run(PumpLoop);

    /// <summary>
    /// Hands a message to the pump for asynchronous processing. Returns immediately; never blocks.
    /// </summary>
    /// <param name="message">The message to enqueue.</param>
    /// <remarks>If the internal channel is full, the oldest unprocessed message is dropped to make room.
    /// This favors caller latency over delivery guarantees under sustained overload.</remarks>
    public static void Pump(ScribeMessage message)
    {
        _channel.Writer.TryWrite(message);
    }

    /// <summary>
    /// Hands multiple messages to the pump for asynchronous processing in order. Returns immediately;
    /// never blocks. Allocates a parameter array per call; prefer single-message <see cref="Pump(ScribeMessage)"/>
    /// for hot paths.
    /// </summary>
    /// <param name="messages">The messages to enqueue, in order.</param>
    public static void Pump(params ScribeMessage[] messages)
    {
        for (int i = 0; i < messages.Length; i++)
            _channel.Writer.TryWrite(messages[i]);
    }

    /// <summary>
    /// Completes the channel and awaits the pump task, ensuring all enqueued messages are processed
    /// before returning. Must be called once during shutdown, typically from a <c>finally</c> block,
    /// and must run <em>before</em> <see cref="DiskManager.ShutdownAsync"/> so that any messages still
    /// in the pump are forwarded to the disk manager's log buffer before that buffer is flushed.
    /// </summary>
    public static async Task ShutdownAsync()
    {
        _channel.Writer.Complete();
        try { await _pumpTask.ConfigureAwait(false); }
        catch (OperationCanceledException) { }
        _cts.Dispose();
    }

    private static async Task PumpLoop()
    {
        var reader = _channel.Reader;
        try
        {
            while (await reader.WaitToReadAsync(_cts.Token).ConfigureAwait(false))
            {
                while (reader.TryRead(out var msg))
                    Process(msg);
            }
        }
        catch (OperationCanceledException) { }
    }

    private static void Process(in ScribeMessage msg)
    {
        var line = Format(msg);
        WriteToConsole(msg.Severity, line);

        if (DiskManager.IsRunning)
            DiskManager.Instance.Log(LogFile.Server, line);   // was EnqueueLogMessage(line)
    }

    private static string Format(in ScribeMessage msg)
    {
        var tag = msg.Severity switch
        {
            ScribeSeverity.Debug => "DEBUG",
            ScribeSeverity.Info => "INFO ",
            ScribeSeverity.Warn => "WARN ",
            ScribeSeverity.Error => "ERROR",
            _ => "?????"
        };

        var stamp = msg.Timestamp.ToString("M/d/yyyy - h:mm tt");
        var line = $"[{stamp}] - [{tag}] - {msg.Message}";

        if (msg.Severity is ScribeSeverity.Warn or ScribeSeverity.Error)
        {
            var fileName = string.IsNullOrEmpty(msg.File)
                ? "?"
                : Path.GetFileName(msg.File);
            line = $"{line}{Environment.NewLine}    at {msg.Member}() in {fileName}:line {msg.Line}";
        }

        if (msg.Exception is not null)
            line = $"{line}{Environment.NewLine}{msg.Exception}";

        return line;
    }

    private static void WriteToConsole(ScribeSeverity severity, string line)
    {
        var prev = Console.ForegroundColor;
        Console.ForegroundColor = severity switch
        {
            ScribeSeverity.Debug => ConsoleColor.DarkGray,
            ScribeSeverity.Info => ConsoleColor.Gray,
            ScribeSeverity.Warn => ConsoleColor.Yellow,
            ScribeSeverity.Error => ConsoleColor.Red,
            _ => prev
        };
        Console.WriteLine(line);
        Console.ForegroundColor = prev;
    }
}

/*
 *------------------------------------------------------------
 * (Scribe.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */