/*
 * (ConfigStore.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 2:25:30 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Newtonsoft.Json;
using SystemTools.Logger;

namespace SystemTools.Storage;

/// <summary>
/// Provides utilities for loading and creating JSON configuration files 
/// with predefined serialization settings.
/// </summary>
public static class ConfigStore
{
    /// <summary>
    /// JSON serializer settings configured for indented output with UTC date 
    /// handling and null value inclusion.
    /// </summary>
    public static readonly JsonSerializerSettings IndentedSettings = new()
    {
        Formatting = Formatting.Indented,
        DateTimeZoneHandling = DateTimeZoneHandling.Utc,
        NullValueHandling = NullValueHandling.Include,
    };

    /// <summary>
    /// JSON serializer settings that include null values, handle dates as UTC,
    /// and uses compact formatting.
    /// </summary>
    public static readonly JsonSerializerSettings CompactSettings = new()
    {
        Formatting = Formatting.None,
        DateTimeZoneHandling = DateTimeZoneHandling.Utc,
        NullValueHandling = NullValueHandling.Include,
    };

    /// <summary>
    /// Loads a JSON configuration file from the specified path, or creates a new 
    /// configuration file with default values if it does not exist.
    /// </summary>
    /// <typeparam name="T">The type of the configuration object. Must have a 
    /// parameterless constructor.</typeparam>
    /// <param name="relativePath">The relative path to the configuration file.</param>
    /// <returns>The loaded configuration object from the file, or a new instance 
    /// with default values if the file does not exist.</returns>
    /// <exception cref="InvalidOperationException">Thrown when the configuration 
    /// file exists but is malformed or deserializes to null.</exception>
    public static T LoadOrCreate<T>(string relativePath) 
        where T : new()
    {
        DiskManager disk = DiskManager.Instance;

        if (disk.FileExists(relativePath))
        {
            string json = disk.ReadTextFile(relativePath);

            try
            {
                T? loaded = JsonConvert.DeserializeObject<T>(json, IndentedSettings);

                return loaded is null
                    ? throw new InvalidOperationException(
                        $"Config file '{relativePath}' deserialized to null.")
                    : loaded;
            }
            catch (JsonException ex)
            {
                throw new InvalidOperationException(
                    $"Config file '{relativePath}' is malformed: {ex.Message}",
                    ex);
            }            
            catch(Exception ex)
            {
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Error,
                    $"Error in LoadOrCreate JSON config file.", ex));
                throw;
            }
        }

        T defaults = new();
        string defaultJson = JsonConvert.SerializeObject(defaults, IndentedSettings);

        disk.WriteTextFile(relativePath, defaultJson);
        disk.FlushAsync();

        return defaults;
    }
}


/*
 *------------------------------------------------------------
 * (ConfigStore.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */