/*
 * (ServerConfig.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 2:58:00 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Newtonsoft.Json;

namespace Stratum.SystemTools.Config;

/// <summary>
/// Represents server configuration settings.
/// </summary>
public sealed class ServerConfig
{
    /// <summary>
    /// Gets or sets the GMUD name.
    /// </summary>
    [JsonProperty("name")]
    public string GMUDName { get; set; } = "Project Stratum";
}



/*
 *------------------------------------------------------------
 * (ServerConfig.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */