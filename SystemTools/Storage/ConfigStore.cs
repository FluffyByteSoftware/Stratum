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
    /// <param name="absolutePath">The absolute path to the configuration file.</param>
    /// <returns>The loaded configuration object from the file, or a new instance 
    /// with default values if the file does not exist.</returns>
    /// <exception cref="InvalidOperationException">Thrown when the configuration 
    /// file exists but is malformed or deserializes to null.</exception>
    /// <remarks>
    /// The miss branch writes defaults into the write-back cache and returns
    /// without forcing a flush. This is deliberate: first-boot config does not
    /// need write-through. The normal 2 s flush cadence persists it, and if
    /// that write fails the defaults are already in memory and returned, with
    /// the failure surfaced through <see cref="DiskManager.HasPersistFailures"/>
    /// and the recovery dump — next boot simply regenerates. A discarded
    /// <c>FlushAsync()</c> here would only look like a guarantee it never gave.
    /// </remarks>
    public static T LoadOrCreate<T>(string absolutePath) 
        where T : new()
    {
        DiskManager disk = DiskManager.Instance;

        if (disk.FileExists(absolutePath))
        {
            string json = disk.ReadTextFile(absolutePath);

            try
            {
                T? loaded = JsonConvert.DeserializeObject<T>(json, IndentedSettings);

                return loaded is null
                    ? throw new InvalidOperationException(
                        $"Config file '{absolutePath}' deserialized to null.")
                    : loaded;
            }
            catch (JsonException ex)
            {
                throw new InvalidOperationException(
                    $"Config file '{absolutePath}' is malformed: {ex.Message}",
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

        disk.WriteTextFile(absolutePath, defaultJson);

        return defaults;
    }
}


/*
 *------------------------------------------------------------
 * (ConfigStore.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */