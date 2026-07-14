/*
 * (Constellations.cs)
 *------------------------------------------------------------
 * Created - 6/15/2026 7:42:52 AM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace SystemTools;


/// <summary>
/// Central home for compile-time constants that never change at runtime,
/// beginning with the canonical filesystem paths every Stratum process
/// resolves against the shared data root. Collapses the path literals that
/// were previously duplicated across each process entry point into a single
/// authoritative source.
/// </summary>
/// <remarks>
/// Every member is a pure constant or a string composed once at type
/// initialization; nothing here touches the disk or depends on
/// <see cref="Storage.DiskManager"/>. As a result these values are safe to
/// read at any point in a process lifetime, including before
/// <c>DiskManager.Initialize</c> has run. Loading the configuration files
/// these paths point at is deliberately not this class's responsibility:
/// each process loads config explicitly at its own call site, after the
/// disk layer is initialized.
/// </remarks>
public static class Constellations
{
    /// <summary>
    /// The shared data root all Stratum processes resolve their storage
    /// against. Passed to <c>DiskManager.Initialize</c> as the first line
    /// of every process entry point.
    /// </summary>
    public const string DataRoot = @"E:\Stratum\data";

    /// <summary>
    /// Full path to the TLS certificate (<c>server.pfx</c>) the LoginServer
    /// presents on its TCP listener.
    /// </summary>
    public static readonly string CertificatePath =
        Path.Combine(DataRoot, "certs", "server.pfx");

    /// <summary>
    /// Full path to the server configuration file (<c>server.json</c>)
    /// backing <see cref="Config.ServerConfig"/>.
    /// </summary>
    public static readonly string ServerConfigPath =
        Path.Combine(DataRoot, "config", "server.json");

    /// <summary>
    /// Full path to the network configuration file (<c>network.json</c>)
    /// backing <see cref="Config.NetworkConfig"/>. Read by the LoginServer
    /// to advertise the Sentinel UDP endpoint and by Sentinel to derive its
    /// UDP bind port from that same advertised endpoint.
    /// </summary>
    public static readonly string NetworkConfigPath =
        Path.Combine(DataRoot, "config", "network.json");

    /// <summary>
    /// Path to the species configuration directory under the data root.
    /// </summary>
    /// <remarks>Contains a trailing directory separator as specified in the 
    /// literal. Value is computed by
    /// combining DataRoot with "game/species/" and may not exist on 
    /// disk.</remarks>
    public static readonly string SpeciesConfigPath =
        Path.Combine(DataRoot, @"game/species");

    /// <summary>
    /// Path to the zones directory under the data root. Each child
    /// directory is one zone, declared by the <c>manifest.json</c> it
    /// contains; ZoneManager's boot scan discovers zones by walking
    /// this directory.
    /// </summary>
    public static readonly string ZonesPath =
        Path.Combine(DataRoot, "zones");
}




/*
 *------------------------------------------------------------
 * (Constellations.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */