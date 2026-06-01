/*
 * (ScribeMessage.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 3:45:16 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Runtime.CompilerServices;

namespace SystemTools.Logger;

/// <summary>
/// An immutable envelope describing a single log entry. Carries the severity, message text, optional
/// exception, and caller source context (member name, file path, line number) captured at the call site.
/// </summary>
/// <remarks>Instances are constructed at the call site and handed to <see cref="Scribe.Pump"/> for
/// asynchronous processing. Caller source context is captured automatically via compiler attributes and
/// surfaced in the formatted output for <see cref="ScribeSeverity.Warn"/> and
/// <see cref="ScribeSeverity.Error"/> entries.</remarks>
/// <remarks>
/// Constructs a message from a severity, a text message, and an optional exception.
/// </remarks>
/// <param name="severity">The severity level of the message.</param>
/// <param name="message">The human-readable message text.</param>
/// <param name="exception">An optional exception to attach. May be <see langword="null"/>.</param>
/// <param name="member">Captured automatically; do not supply explicitly.</param>
/// <param name="file">Captured automatically; do not supply explicitly.</param>
/// <param name="line">Captured automatically; do not supply explicitly.</param>
public readonly struct ScribeMessage(
    ScribeSeverity severity,
    string message,
    Exception? exception,
    [CallerMemberName] string member = "",
    [CallerFilePath] string file = "",
    [CallerLineNumber] int line = 0)
{
    /// <summary>
    /// The local time at which the message was constructed.
    /// </summary>
    public DateTime Timestamp { get; } = DateTime.Now;

    /// <summary>
    /// The severity level of the message.
    /// </summary>
    public ScribeSeverity Severity { get; } = severity;

    /// <summary>
    /// The human-readable message text. When constructed from an exception alone, this mirrors
    /// <see cref="System.Exception.Message"/>.
    /// </summary>
    public string Message { get; } = message;

    /// <summary>
    /// The name of the member (method, property, etc.) that constructed the message, captured via
    /// <see cref="CallerMemberNameAttribute"/>.
    /// </summary>
    public string Member { get; } = member;

    /// <summary>
    /// The full source file path of the call site, captured via <see cref="CallerFilePathAttribute"/>.
    /// Only the file name portion is rendered in formatted output.
    /// </summary>
    public string File { get; } = file;

    /// <summary>
    /// The source line number of the call site, captured via <see cref="CallerLineNumberAttribute"/>.
    /// </summary>
    public int Line { get; } = line;

    /// <summary>
    /// The exception associated with this message, or <see langword="null"/> if none was provided.
    /// </summary>
    public Exception? Exception { get; } = exception;

    /// <summary>
    /// Constructs a message from a severity and a text message. Caller source context is captured automatically.
    /// </summary>
    /// <param name="severity">The severity level of the message.</param>
    /// <param name="message">The human-readable message text.</param>
    /// <param name="member">Captured automatically; do not supply explicitly.</param>
    /// <param name="file">Captured automatically; do not supply explicitly.</param>
    /// <param name="line">Captured automatically; do not supply explicitly.</param>
    public ScribeMessage(
        ScribeSeverity severity,
        string message,
        [CallerMemberName] string member = "",
        [CallerFilePath] string file = "",
        [CallerLineNumber] int line = 0)
        : this(severity, message, null, member, file, line) { }

    /// <summary>
    /// Constructs a message from a severity and an exception. The exception's
    /// <see cref="System.Exception.Message"/> is used as the displayed text.
    /// </summary>
    /// <param name="severity">The severity level of the message.</param>
    /// <param name="exception">The exception to log. Must not be <see langword="null"/>.</param>
    /// <param name="member">Captured automatically; do not supply explicitly.</param>
    /// <param name="file">Captured automatically; do not supply explicitly.</param>
    /// <param name="line">Captured automatically; do not supply explicitly.</param>
    public ScribeMessage(
        ScribeSeverity severity,
        Exception exception,
        [CallerMemberName] string member = "",
        [CallerFilePath] string file = "",
        [CallerLineNumber] int line = 0)
        : this(severity, exception.Message, exception, member, file, line) { }
}

/*
 *------------------------------------------------------------
 * (ScribeMessage.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */