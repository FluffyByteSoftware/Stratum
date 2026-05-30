/*
 * (ConfigStore.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 2:25:30 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Newtonsoft.Json;
using Stratum.SystemTools.Logger;

namespace Stratum.SystemTools.Storage;

public static class ConfigStore
{
    public static readonly JsonSerializerSettings IndentedSettings = new()
    {
        Formatting = Formatting.Indented,
        DateTimeZoneHandling = DateTimeZoneHandling.Utc,
        NullValueHandling = NullValueHandling.Include,
    };

    public static readonly JsonSerializerSettings VerboseSettings = new()
    {
        Formatting = Formatting.None,
        DateTimeZoneHandling = DateTimeZoneHandling.Utc,
        NullValueHandling = NullValueHandling.Include,
    };

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
                Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                    $"Error in LoadOrCreate JSON config file.", ex));
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