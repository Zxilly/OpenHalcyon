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

## Match transport

The [protocol crate](../crates/protocol/src/lib.rs) implements length framing,
handshake messages, fixed-width fields, encrypted blocks, replay container
boundaries and skin name hashes. [Reader metadata](../crates/protocol/src/metadata.rs)
records known payload layouts and ignored tags. The game service interprets
evidenced commands and emits snapshots, visibility-filtered updates and purchase
acknowledgements. Uninterpreted payloads remain available to the offline reader.
The reference checks decoded 43,160 records across 14 VGR chunks and checked
encrypted messages from three independent sessions.

The Android client selected an actual hero and skin and displayed **LOCKED IN**.
The local CE capture contains subsequent client hellos, but no matching server
acknowledgement or initial world snapshot for a successful handoff. Historical
ARAL logs truncate frames to 48 bytes. A VGR replay starts inside the game and
does not encode the live CE draft-to-world negotiation. The native transition
to the battlefield, touchscreen actions there and the return to the menu after
a complete native match remain open. The isolated emulator run and the scripted
game-socket tests measure different stages.

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
