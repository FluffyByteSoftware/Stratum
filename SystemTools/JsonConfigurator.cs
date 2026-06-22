/*
 * (JsonConfigurator.cs)
 *------------------------------------------------------------
 * Created - 6/20/2026 9:01:22 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using System.Text.Json;
using System.Text.Json.Serialization;

namespace SystemTools;

/// <summary>
/// Shortcut global to quickly pull various JsonSerializerOptions for different uses.
/// The options are immutable and readonly, so they can be shared across the app.
/// </summary>
public static class JsonConfigurator
{
    /// <summary>
    /// Default JsonSerializerOptions for content being properly / 
    /// matching capitalized, indented, and enums converted.
    /// </summary>
    public static readonly JsonSerializerOptions ContentIndented = new()
    {
        PropertyNameCaseInsensitive = true,
        WriteIndented = true,
        Converters = { new JsonStringEnumConverter() },
    };

    /// <summary>
    /// Default JsonSerializerOptions for content being properly / 
    /// matching capitalized, and enums converted, but not indented.
    /// </summary>
    public static readonly JsonSerializerOptions Content = new()
    {
        PropertyNameCaseInsensitive = true,
        WriteIndented = false,
        Converters = { new JsonStringEnumConverter() },
    };

    /// <summary>
    /// Default JsonSerializeOptions for content being CamelCased, 
    /// and enums converted.
    /// </summary>
    public static readonly JsonSerializerOptions Persistence = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        Converters = { new JsonStringEnumConverter() }
    };

}


/*
 *------------------------------------------------------------
 * (JsonConfigurator.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */