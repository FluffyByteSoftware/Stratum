/*
 * (ScribeSeverity.cs)
 *------------------------------------------------------------
 * Created - 5/10/2026 3:45:16 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools.Logger;

/// <summary>
/// Specifies the severity level for log messages.
/// </summary>
/// <remarks>Use this enumeration to indicate the importance or type of a log entry, such as debugging
/// information, general informational messages, warnings, or errors. The severity level can be used to filter or
/// categorize log output.</remarks>
public enum ScribeSeverity
{
    /// <summary>
    /// Represents a log level used to indicate detailed information that is useful for debugging purposes.
    /// </summary>
    Debug,
    /// <summary>
    /// Represents a log level used to indicate general informational message.
    /// </summary>
    Info,
    /// <summary>
    /// Represents a log level used to indicate a potentially harmful situation, exposing an exception or a message or both.
    /// </summary>
    Warn,
    /// <summary>
    /// Represents a log level used to indicate a harmful situation, exposing an exception or a message or both.
    /// </summary>
    Error,
}



/*
 *------------------------------------------------------------
 * (ScribeSeverity.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */