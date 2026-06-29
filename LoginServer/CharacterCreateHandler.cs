/*
 * (CharacterCreateHandler.cs)
 *------------------------------------------------------------
 * Created - 6/29/2026
 * Created by - Seliris
 *-------------------------------------------------------------
 */

using Shared.Networking.Packets.Character;
using Networking.Tcp;
using SystemTools.Characters;
using SystemTools.Logger;
using System;
using System.Threading.Tasks;
using Shared.Networking;

namespace LoginServer;

/// <summary>
/// Handles the player-driven character-create exchange on the character channel:
/// resolves the owning account from the authenticated connection, runs
/// <see cref="CharacterCreator"/>, and answers with a
/// <see cref="CharacterCreateResponsePacket"/>.
/// </summary>
/// <remarks>
/// This handler is the consumer of the <see cref="AuthOutcome.NeedsCharacter"/>
/// outcome: a connection that authenticated without a character is left open and
/// registered in the shared <see cref="CharacterLoginRegistry"/>, and the create
/// request arrives here on that same connection. The owning account is taken from the
/// registry entry keyed by the connection, never from the packet — the connection's
/// completed authentication is the trust anchor, so a create packet on a connection
/// with no registry entry never authenticated into the character-login phase and is
/// rejected outright. Terminal outcomes clear the registry entry and disconnect, since
/// the attempt is finished (on <see cref="CharacterCreateOutcome.Created"/> the client
/// re-authenticates on a fresh connection and now lands <see cref="AuthOutcome.Ok"/>).
/// The two retryable name rejections leave the connection open and the entry intact so
/// the player can correct the name and send another create without a reconnect — the
/// connection was kept alive after <c>NeedsCharacter</c> precisely to avoid that
/// reconnect, and a name retry is a human-speed event in a once-per-account flow.
/// </remarks>
/// <param name="characterLogins">The shared registry binding character-less
/// authenticated connections to their accounts, populated by <see cref="AuthHandler"/>
/// on the <see cref="AuthOutcome.NeedsCharacter"/> branch.</param>
public sealed class CharacterCreateHandler(CharacterLoginRegistry characterLogins)
{
    /// <summary>
    /// The host used to send the response back on the originating connection. Assigned
    /// after construction once the host exists, closing the null window before any
    /// connection can arrive.
    /// </summary>
    public TcpHost? Host { get; set; }

    /// <summary>
    /// Handles an incoming character-create request on an authenticated, character-
    /// less connection.
    /// </summary>
    /// <param name="conn">The connection the request arrived on, whose
    /// <see cref="TcpConnection.Id"/> keys the owning account in the registry.</param>
    /// <param name="packet">The create request carrying the raw, player-typed
    /// name.</param>
    /// <returns>A task representing the asynchronous handling operation.</returns>
    /// <remarks>
    /// A request on a connection with no registry entry is treated as a protocol
    /// violation — the connection never reached <see cref="AuthOutcome.NeedsCharacter"/>
    /// — and the connection is dropped without invoking the create service. Otherwise
    /// the resolved account and the packet's name are handed to
    /// <see cref="CharacterCreator.Create"/>, the result is mapped to its wire form,
    /// the response is sent, and the connection is torn down unless the outcome is a
    /// retryable name rejection.
    /// </remarks>
    public async ValueTask OnCharacterCreate(
        TcpConnection conn, CharacterCreateRequestPacket packet)
    {
        if (!characterLogins.TryGet(conn.Id, out var session))
        {
            Scribe.Pump(new ScribeMessage(ScribeSeverity.Warn,
                $"Character create on unregistered connection {conn.Id} "
                + $"from {conn.RemoteEndPoint}; dropping."));

            conn.RequestDisconnect(SecureDisconnectReason.InvalidUserCredentials);
            return;
        }

        var result = CharacterCreator.Create(
            session.AccountId, packet.RequestedName, out var characterName);

        LogOutcome(conn, session.AccountId, characterName, result);

        var outcome = ToOutcome(result);
        var response = new CharacterCreateResponsePacket(outcome);

        var host = Host
            ?? throw new InvalidOperationException("Host not assigned.");

        await host.SendAsync(conn, response).ConfigureAwait(false);

        if (!IsRetryable(outcome))
        {
            characterLogins.Remove(conn.Id);

            var reason = outcome == CharacterCreateOutcome.Created
                ? SecureDisconnectReason.None
                : SecureDisconnectReason.CharacterCreateFailed;

            conn.RequestDisconnect(reason);
        }
    }

    /// <summary>
    /// Determines whether an outcome leaves the connection open for another attempt.
    /// </summary>
    /// <param name="outcome">The outcome to classify.</param>
    /// <returns><see langword="true"/> for the two name rejections the player can fix
    /// in place; otherwise <see langword="false"/>.</returns>
    /// <remarks>
    /// Only <see cref="CharacterCreateOutcome.NameInvalid"/> and
    /// <see cref="CharacterCreateOutcome.NameTaken"/> are retryable: both are correctable
    /// by choosing a different name on the still-open connection. Every other outcome —
    /// success, the account-state anomalies, a persist fault, or the unwritten
    /// <see cref="CharacterCreateOutcome.None"/> sentinel — is terminal for this
    /// connection and falls through to teardown.
    /// </remarks>
    private static bool IsRetryable(CharacterCreateOutcome outcome) =>
        outcome is CharacterCreateOutcome.NameInvalid
                or CharacterCreateOutcome.NameTaken;

    /// <summary>
    /// Maps the service-internal <see cref="CharacterCreateResult"/> to its
    /// <c>Shared</c>-side wire form.
    /// </summary>
    /// <param name="result">The result returned by
    /// <see cref="CharacterCreator.Create"/>.</param>
    /// <returns>The corresponding <see cref="CharacterCreateOutcome"/>.</returns>
    /// <remarks>
    /// The mapping is an explicit switch rather than a numeric cast even though the two
    /// enums mirror each other one-for-one today: the wire enum lives in <c>Shared</c>
    /// and the service enum in <c>SystemTools</c>, and an explicit map keeps the wire
    /// contract from shifting silently if either side is ever reordered or extended. An
    /// unmapped result — including <see cref="CharacterCreateResult.None"/>, which a
    /// completed call never returns — collapses to <see cref="CharacterCreateOutcome.None"/>,
    /// the loud-failure sentinel the client reads as a corrupt or uninitialized
    /// outcome.
    /// </remarks>
    private static CharacterCreateOutcome ToOutcome(CharacterCreateResult result) =>
        result switch
        {
            CharacterCreateResult.Created
                => CharacterCreateOutcome.Created,
            CharacterCreateResult.NameInvalid
                => CharacterCreateOutcome.NameInvalid,
            CharacterCreateResult.AccountNotFound
                => CharacterCreateOutcome.AccountNotFound,
            CharacterCreateResult.AccountAlreadyHasCharacter
                => CharacterCreateOutcome.AccountAlreadyHasCharacter,
            CharacterCreateResult.NameTaken
                => CharacterCreateOutcome.NameTaken,
            CharacterCreateResult.PersistFailed
                => CharacterCreateOutcome.PersistFailed,
            _ => CharacterCreateOutcome.None,
        };

    /// <summary>
    /// Logs the create outcome at a severity matching its nature: routine for user
    /// name errors, warning for account-state anomalies, error for server faults.
    /// </summary>
    /// <param name="conn">The originating connection, for its remote endpoint.</param>
    /// <param name="accountId">The owning account.</param>
    /// <param name="characterName">The created character's canonical name on success;
    /// otherwise empty.</param>
    /// <param name="result">The result to log.</param>
    /// <remarks>
    /// Name rejections are expected player error and logged at
    /// <see cref="ScribeSeverity.Debug"/> to avoid noise. The account-state cases are
    /// not expected in this flow — the connection authenticated and reached
    /// <see cref="AuthOutcome.NeedsCharacter"/>, so the account existed and was
    /// character-less moments earlier — so reaching them implies a race or drift and is
    /// logged at <see cref="ScribeSeverity.Warn"/>. A persist fault and any unmapped
    /// result are server-induced and logged at <see cref="ScribeSeverity.Error"/>.
    /// </remarks>
    private static void LogOutcome(
        TcpConnection conn,
        string accountId,
        string characterName,
        CharacterCreateResult result)
    {
        var (severity, message) = result switch
        {
            CharacterCreateResult.Created => (ScribeSeverity.Info,
                $"Character '{characterName}' created for account "
                + $"'{accountId}' from {conn.RemoteEndPoint}."),

            CharacterCreateResult.NameInvalid => (ScribeSeverity.Debug,
                $"Create rejected (invalid name) for account "
                + $"'{accountId}' from {conn.RemoteEndPoint}."),

            CharacterCreateResult.NameTaken => (ScribeSeverity.Debug,
                $"Create rejected (name taken) for account "
                + $"'{accountId}' from {conn.RemoteEndPoint}."),

            CharacterCreateResult.AccountNotFound => (ScribeSeverity.Warn,
                $"Create found no account '{accountId}' "
                + $"(from {conn.RemoteEndPoint})."),

            CharacterCreateResult.AccountAlreadyHasCharacter
                => (ScribeSeverity.Warn,
                $"Create for account '{accountId}' which already has a "
                + $"character (from {conn.RemoteEndPoint})."),

            CharacterCreateResult.PersistFailed => (ScribeSeverity.Error,
                $"Create for account '{accountId}' failed to persist "
                + $"(from {conn.RemoteEndPoint})."),

            _ => (ScribeSeverity.Error,
                $"Create for account '{accountId}' returned unexpected "
                + $"result {result} (from {conn.RemoteEndPoint})."),
        };

        Scribe.Pump(new ScribeMessage(severity, message));
    }
}


/*
 *------------------------------------------------------------
 * (CharacterCreateHandler.cs)
 * See License.txt for licensing information.
 *-----------------------------------------------------------
 */