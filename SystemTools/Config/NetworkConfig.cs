/*
 * (NetworkConfig.cs)
 *------------------------------------------------------------
 * Created - 5/30/2026 2:59:13 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Newtonsoft.Json;

namespace SystemTools.Config;

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

    /// <summary>
    /// The UDP endpoint (host:port) of the Sentinel front door, advertised to
    /// clients after successful TCP authentication so they know where to send
    /// the session token. Must match the address and port Sentinel binds.
    /// </summary>
    [JsonProperty("advertisedUdpEndpoint")]
    public string AdvertisedUdpEndpoint { get; set; } = "10.0.0.84:9998";
}



/*
 *------------------------------------------------------------
 * (NetworkConfig.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */