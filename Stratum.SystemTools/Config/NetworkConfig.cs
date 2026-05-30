/*
 * (NetworkConfig.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 2:59:13 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Newtonsoft.Json;

namespace Stratum.SystemTools.Config;

/// <summary>
/// Represents network configuration settings for a TCP server.
/// </summary>
public sealed class NetworkConfig
{
    /// <summary>
    /// Address of the IP for our Tcp Server.
    /// </summary>
    [JsonProperty("bindAddress")]
    public string BindAddress { get; set; } = "10.0.0.84";

    /// <summary>
    /// Port our Tcp Server listens on.
    /// </summary>
    [JsonProperty("port")]
    public int Port { get; set; } = 9997;
}



/*
 *------------------------------------------------------------
 * (NetworkConfig.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */