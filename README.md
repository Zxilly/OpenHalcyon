# OpenHalcyon

OpenHalcyon is an independent Rust server project for the Vainglory Community Edition Android client (build 147219). Operators run their own instances. The design supports single-host games and matchmaking between trusted peers.

The workspace targets stable Rust. The MIT license covers the project's original implementation. Supply any client-derived data locally.

## What works today

The original Android client can create or recover a guest, queue for 5v5 Solo
Bots, select a hero and skin, and load the battlefield. On build 147219 an
isolated client displayed the ten-player loading screen, build selection, the
battlefield and HUD. Moving the touchscreen joystick sent both movement
opcodes on the original TCP connection. The native Solo opening uses measured
hero, crystal, mid-tower and minion positions. A device run verified movement
from its starting pad and later ended in a native crash. Native reentry and
the victory screen remain open. [CE compatibility](docs/protocol-coverage.md) · [Solo mid lane](docs/solo-lane.md)

The Rust game host runs a match through crystal destruction and settlement.
An integration test starts three independent server processes, signs two CE
guests into separate home nodes over HTTPS, and opens their encrypted game
sockets on the third node. Both guests return to menus with one identical
result after an election during play. A separate host-restart test verifies
that losing the room aborts the match on both homes. [Process tests](crates/server/tests/support/process_pool.rs)

Six encrypted ARAL protocol clients can play on a lane reconstructed from seven
archived matches. The simulation uses the recorded team pads, tower and crystal
positions and staggered waves. Idris and Lorelai accept their evidenced native
skill-learning and casting requests. Their combat rules describe which parameters
come from published data and which are local tuning.
[ARAL](docs/aral.md) · [Idris](crates/sim/idris.md) · [Lorelai](crates/sim/lorelai.md)

## Run an instance

```sh
cargo build --locked --release -p openhalcyon
target/release/openhalcyon init --dir runtime --host localhost
target/release/openhalcyon serve --config runtime/server.toml
```

On Windows the executable is `target\release\openhalcyon.exe`. Set `--host` to a name or IPv4 address that the client can reach. An Android emulator on the same workstation can use `10.0.2.2`. Initialization writes a local TLS certificate, signing key and one-time copy of the generated admin password into the instance directory. Set file permissions for the service account. For a public installation, configure a certificate trusted by your clients, the advertised host, and the dashboard's exact HTTPS origin.

The bootstrap endpoint is `http://HOST:8080/kindred/live/147219-status-redirect`. It advertises the HTTPS platform on port 8443. Notifications use port 2112. Newly initialized instances also advertise XMPP with STARTTLS on port 5222. Sign in to `https://HOST:8443/admin` as `admin` using `initial-admin-password`. Game rooms advertise their own TCP ports.

Paths in `server.toml` are relative to that file. For a local research installation set `data_dir` to the directory containing `units.json`, `items.json` and `entity_manifest.json`, and `manifests_dir` to the directory containing the archived talents and skin manifest responses. Place these top-level keys before any TOML section. Only manifest payloads are consumed from those two responses. Keep private captures outside the checkout. With no external data the simulation uses built-in unit statistics. Native Solo and legacy ARAL use their measured mid-lane layouts. Other queues use the validation layout.

An operator with a decoded draft trace can export the catalog once. Run
`openhalcyon export-catalog --trace /private/match_decoded.jsonl > /srv/game-data/catalog.json`.
Instances load this ordered list of 0x0453 hero and skin names from their
`data_dir`. It produces the same federation content digest as the original
catalog records. Older research directories can still supply the adjacent
decoded trace. Keep the original trace on the research machine.

To forward a fixed range through a firewall, set the top-level value
`game_port_range = { first = 7100, last = 7131 }`. Forward those TCP ports
to the same numbers on the host. `max_rooms` limits simultaneous admissions.
Each room has one listener, and active spectators use additional listeners
from the same range. Optional `[admission]` thresholds cover `tick_p99_ms`,
`process_cpu_percent`, `rss_bytes` and `egress_bytes_per_second`. A CPU value
of 100 is one logical processor. The dashboard reports the sampled values and
whether the host is accepting new games. Set top-level `replay_dir` to enable
private VGR recording.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo run --release -p halcyon-sim --example lane_bench -- /path/to/delivery
```

The TCP codec and replay parser live in `crates/protocol`, the CE JSON-RPC and notification handlers in `crates/ce-platform`, and guest persistence in `crates/platform`. Game connections deliver commands to `crates/sim` through `crates/game`. Match allocation, replicated state and federation integration live in `crates/matchmaking`, `crates/federation` and `crates/peer-play`. The administrative router and dashboard are in `crates/admin`.

- [Runtime, bots and peer matchmaking](docs/runtime-and-federation.md)
- [Player REST, long polling and WebSocket events](docs/player-api.yaml)
- [Management API](docs/admin-api.v1.yaml)
- [Run a three-node pool](docs/operating-a-pool.md)
- [Configure inventory and game currency](docs/local-content.md)
- [Configure operator attestations](docs/provider-attestations.md)
- [Chat protocol](crates/chat/README.md)
- [Simulation and bot rules](crates/sim/README.md)
- [Load test](tools/bench/README.md)
- [CE protocol coverage and client verification](docs/protocol-coverage.md)
- [Game messages and known fields](docs/game-wire.md)
