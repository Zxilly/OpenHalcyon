# CE build 147219 compatibility

## Platform

The [RPC inventory](ce-rpc.tsv) lists 155 candidate method names from the Android
call sites, the local protocol specification and the iOS parser inventory. Each
row records the source of the name, the reported parser family when available,
and the implementation that handles it. The Android relay recorded 14 distinct
methods. A captured name establishes use of that method on the wire. A static
call site or parser family supplies less information about argument order,
optional fields and return types. Locally defined behaviors for these methods
need further client comparison.

Fields found beside an RPC call can also look like methods. For example,
`startSessionUrl` and `partyMembers` appear in response objects, and
`production` identifies the environment. The inventory excludes these strings.
The `charm` and `social_ping_pack` entries are compatibility aliases for
equipment slots. The catalog test dispatches every listed name through the HTTPS
router's handler using an authenticated session. It checks routing and failure
classification. Focused lifecycle tests exercise successful requests and
committed state changes.

| Path | Evidence and verification |
| --- | --- |
| HTTP bootstrap, guest creation, guest recovery, session start | Original Android CE running in an isolated emulator connected to the Rust instance, reached the main menu and recovered the same account after restart. |
| Solo Bot queue | Android client joined, received a room address, authenticated to the local game endpoint and displayed a native 5v5 roster. |
| Cross-instance parties | Three TLS-authenticated voters, party members on one home and a solo player on another, served by a third node. Integration tests cover individual acceptance, the game socket, team allocation, settlement and independent exits. |
| Spectators | A confirmed friend on the player's home can watch a local or peer-hosted match through a dedicated read-only listener. A three-voter test covers roster ownership, snapshot delivery, revocation on visibility changes, and cleanup at settlement. Native spectator admission and friendships registered on different homes still need verification. |
| Items, guilds, quests and accounts | SQLite transactions, permission and retry tests exercise the local rules. Operator-defined exchanges specify actual costs and rewards. Some archived methods have only static schema evidence. |

The archived RPC traffic includes 15 successful `acceptMatch` calls with a second string argument. Eleven used a match UUID and four used an empty string; each returned boolean `true`. The CE handler now passes the supplied match ID to the platform's transactional offer check. A mismatched offer leaves the current match untouched. The observed empty-string form resolves the current offer. The existing session-only local form retains its session-envelope response.

## Match transport

The [protocol crate](../crates/protocol/src/lib.rs) implements length framing,
handshake messages, fixed-width fields, encrypted blocks, replay container
boundaries and skin name hashes. [Reader metadata](../crates/protocol/src/metadata.rs)
records known payload layouts and ignored tags. The game service interprets
evidenced commands and emits snapshots, visibility-filtered updates and purchase
acknowledgements. Uninterpreted payloads remain available to the offline reader.
The reference checks decoded 43,160 records across 14 VGR chunks and checked
encrypted messages from three independent sessions.

The [game opcode inventory](ce-game.tsv) contains every entry in the Android
table and both field-decoding counts for the local replay corpus. The table
specifies 122 frame sizes, 48 branches ignored by the Android receiver and one
special clock branch. An ignored receiver branch can still identify a command
in the opposite direction. The 14 replay chunks contain 56 distinct opcodes.
A typed count requires at least one decoded variant, including variants that
only establish a prefix. Zero means that these chunks contain no example of
that class. Every record with a fixed-size prefix passed its size check.
Callers retain every byte after the prefix, including bytes of unknown meaning.

To reproduce the inventory for another recording, run
`cargo run --locked -p halcyon-protocol --example replay_inventory -- <chunk-directory>`.
It reads local VGR files and writes aggregate counts to standard output.

The separate Android draft exchange includes player identification (0x03E8)
in the client-to-server direction, setup and mode (0x03E9, 0x0454) from the
server, 275 server catalog records (0x0453), and two server rosters (0x0459).
That exchange includes 0x0458, 0x046B, 0x045E and 0x0455 in both directions.
These counts describe a single recorded exchange. The VGR counts describe
the in-game timeline and exclude draft traffic.

In an isolated Android 147219 Solo Bots run the server sent ten full 0x03EE
rosters and 0x046C after lock. The client displayed a ten-player VS screen,
including Dark Parade Adagio in the selected seat. After loading assets it
sent 0x046E and 0x0471 on the existing connection. The request contained the
reserved actor and flag 1. The server replied with the native mode, rosters,
clock, hero spawns, the two acknowledgements and a 0x045C seat table.
The client opened Choose a Build and rendered the battlefield after selection.
A touchscreen joystick swipe produced 0x03F4 and 0x03F5 on the same socket.
The host also supports a separately authenticated data connection.

A second Android run selected Ardan from a ten-hero local catalog and rendered
the allied crystal, hero and HUD. The server-side VGR placed actor 1500 at
(0.0000, -1.5000) in the opening spawn. After a touchscreen swipe, its first
position update arrived at game time 9.45 seconds at (0.1689, -1.4025).
There were 26 authoritative updates through 10.70 seconds. The last recorded
position was (3.788, 0.687). This client disconnected at 28.9 seconds and
authenticated again. The room eventually aborted and the client returned to the
main menu. This run did not display the native victory or reward screens.

The fresh-connection path also sends the seated actor and mode in a world-form
0x03E9 before 0x046C. A loopback socket test resumes the room and checks the
view against team visibility. In a separate Android run a normal app restart
from the rendered battlefield recovered the guest and authenticated fresh
control sockets. No second world session or rendered battlefield followed on
the device. Native reentry and a complete native victory screen remain open.

A later Android 147219 run used the measured Solo mid-lane opening. Actor 1500
started at X/Z (−90, 0). A touchscreen swipe produced 23 authoritative position
updates, from (−89.8311, 0.0975) at 8.30 simulated seconds to
(−86.3607, 2.1012) at 9.40 seconds. The client rendered its hero, HUD and
friendly base in the map. The first connection ended about 38.2 seconds after
the simulation started. Android's process history reports a native crash.
Four fresh control sockets authenticated to the same room. The per-connection
log recorded no new scene request on them. A native client victory screen
and a rendered native rejoin are still unverified. The server binary used
for this check had SHA-256
`58570c9ef5f89f86329851e84b1f1dad38057015dd31cf2ce8a8e1b8c6e021c7`.
The six-socket ARAL tests verify server-side delivery independently.
[Measured Solo mid lane](solo-lane.md) · [ARAL settlement](aral.md)

The nine historical TCP archives supply full-sized encrypted frames, including
seven ARAL matches. Their decoded text previews only preserve prefixes. The
offline inventory validated 1,824 client frames and 398,293 server frames after
the plain handshakes. The archives do not identify the originating build.
[Live TCP coverage](live-captures.md) · [ARAL scene negotiation](aral.md)

The static iOS inventory was checked against the local HackedGlory research
archive at commit `cf2cc57016f0460cc9404031f4a2b7653af6ebaf`. Historical
Android screenshots show a rendered official-session world and result overlay,
but provide no packet fields or transition ordering. Private recordings,
account tokens, client binaries and asset packs stay in the operator's local
research directory.

## Local VGR export

Set `replay_dir` to a private directory to record the first human seat's admitted game socket. The process creates a subdirectory for each room and each connection generation. A reconnect begins another segment. On Unix the replay root must have owner-only permissions. On Windows it inherits the operator-supplied directory ACL. Retention is controlled by the operator.

Each record contains big-endian simulated seconds (f32), payload length including opcode (u32), opcode (u16), and the unpadded message body. The timestamp is sampled when that socket finishes writing its batch. Only audited outbound gameplay opcodes are exported. Player roster names and UUIDs are removed from both roster layouts. The export represents the first seat's fog-of-war view; socket handshakes, control traffic, incoming actions and internal simulation events are outside the stream.

Every 200 ticks, a closed, uninterrupted ten-second window is published as `NNNNNNNN.vgr`. An open window, a segment beginning partway through a window, or the trailing window retains the suffix `.vgr.partial`. A process failure may leave a truncated final record in a partial file. Continuation chunks contain the delivered deltas and can be read in order starting at the opening segment. Independent playback from each chunk and stock-client playback have not been verified.

The async writer accepts at most eight batches of 64 KiB for each recorded connection. A 128 MiB per-match budget includes all reconnects. A full queue, exhausted budget or filesystem error stops that recording while the game continues. The service emits a warning for this condition.
## Captured Android request and response shapes

The standalone Android request log and relay response log contain the following counts of complete JSON records. A response counts only when its envelope reports success. Counts refer to records, including repeated polling. The Android router test supplies synthetic arguments of the captured types and exercises guest recovery, menu, queue, match assignment, exit and logout. The static skin manifest is a JSON string, while the talents catalog is a JSON object.

| Method | Requests | Successful responses | Return-value shape |
| --- | ---: | ---: | --- |
| createGuestPlayer | 2 | 2 | Guest credential and start URL strings |
| getPlayerForGuestAccount | 5 | 4 | Guest credential and start URL strings |
| startSessionForPlayer | 4 | 4 | Menu state, platform and notification addresses, configuration, player data and integer counts |
| getPlayerInfo | 8 | 9 | Player profile or state update, depending on captured response |
| getSkinManifest | 18 | 18 | String |
| getTalentsData | 4 | 4 | Object keyed by talent |
| joinLobby | 8 | 8 | Success boolean and integer TTL. The two ARAL joins also returned queue state and player count. |
| update | 300 | 300 | State, versions, fractional epoch expiry and integer TTL, plus state-dependent fields |
| exitLobby | 6 | 5 | State and versions |
| updatePlatformPlayerConfig | 1 | 1 | Empty object |
| notifyExitPostMatch | 2 | 2 | Success boolean |
| friendListAll | 1 | 1 | Confirmed and pending arrays, integer offline count |
| recordMatchExperienceMetrics | 1 | 1 | Empty object |
| endSession | 1 | 1 | Menu state with an empty envelope session token |

The local session advertises empty experiments, ad identifiers and support-content location where the instance has no such services configured. These fields preserve the observed JSON types. Production experiment assignments and vendor content are outside the local runtime.

### Party notifications

Set the top-level `native_state_updates = true` in the server TOML to advertise the native state subscription. It defaults to false. Configure `notify_listen` and `public_host` so clients on the intended network can reach the advertised WebSocket and HTTP fallback endpoints. Both URLs include a session-scoped bearer grant minted at bootstrap. Keep these URLs private and obtain fresh URLs by bootstrapping after a server restart. The notification listener uses plaintext HTTP and WebSocket.

WebSocket and HTTP long polling use the same recipient-local SQLite cursor. To resume, send the previous envelope's `time` and `tag` as query parameters. A request without a cursor starts with the current player snapshot. Invitations go to the invitee alone, roster changes go to party members, and kicks go to the removed player. A departure is sent to the remaining members. Dissolving the party sends a state-change notice to its former members. Retries of an existing invitation or an already accepted invitation add no duplicate events. Revoked invitations and obsolete roster notices replay as a generic state snapshot at the original cursor.

The 155-method inventory establishes the available party RPC names. The local protocol notes (§4) and the generated `stateUpdateCallback` analysis identify the envelope and the four `quintParty*` keys in `text`. This server emits those keys as markers with empty objects. The generated reports and demonstration server suggest inner fields, but the original values and native UI behavior remain unverified. Details are available from the authorized party RPCs.
