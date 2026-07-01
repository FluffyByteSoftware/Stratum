/*
 * (Channel.cs)
 *------------------------------------------------------------
 * Created - 5/22/2026 5:34:18 PM
 * Created by - Seliris
 *-------------------------------------------------------------
 */

namespace Shared.Networking;

/// <summary>
/// Specifies the type of communication channel.
/// </summary>
public enum Channel : byte
{
    /// <summary>
    /// This channel is explicitly used for Authentication.
    /// </summary>
    Auth = 0x00,
    /// <summary>
    /// This channel is explicitly used for Lifecycle management, such as 
    /// server registration, heartbeat, and other lifecycle-related operations.
    /// </summary>
    Lifecycle = 0x01,
    /// <summary>
    /// This channel is explicitly used for receiving packets from clients, 
    /// such as game data, commands, and other client-originated communications.
    /// </summary>
    Client = 0x02,
    /// <summary>
    /// This channel is explicitly used for cluster-related communications, 
    /// such as inter-server messages, synchronization, and other cluster operations.
    /// </summary>
    Cluster = 0x03,
    /// <summary>
    /// This channel is explicitly used for administration purposes.oh of 
    /// </summary>
    Admin = 0x04
}


/*
*------------------------------------------------------------
* (Channel.cs)
* See License.txt for licensing information.
*-----------------------------------------------------------
*/