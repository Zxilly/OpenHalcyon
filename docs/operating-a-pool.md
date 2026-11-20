# Operating a matchmaking pool

Each operator runs the same server executable and keeps a separate instance UUID,
platform database, federation journal, Raft database and private keys. Players sign
in to their own instance. The elected Raft leader orders ticket and room decisions.
Any voter can accept local players, submit a proposal, win the next election or host
a match. Movement and combat stay on the room host.

## Configure three instances

1. Run `openhalcyon init` in an empty instance directory on each machine. Use each
   machine's reachable public hostname. Keep the three generated `instance_id` values.
2. Agree on the complete initial voting membership and a pool CA. Issue each node a distinct
   TLS leaf certificate covering the DNS name or IP in its advertised peer address.
   If the leaf includes extended key usages, include both client and server authentication.
   Each operator keeps its own leaf private key. Exchange the CA and public leaf
   certificates so that every member can pin the same node identities.
3. Add the following section to each instance's `server.toml`. The latency budget
   is in milliseconds. This example uses automatic measurements.

```toml
[federation]
cluster_file = "pool.json"
journal = "peer-play.sqlite"
max_latency_ms = 120
```

4. Write `pool.json` on each node. The example below belongs to node A. On nodes B
   and C, use their own `node_id`, `cert` and `key`. All three use `"bootstrap": "auto"`
   and the same complete `peers` map, including themselves. Each certificate path can
   differ as long as it contains the same pinned public leaf. The TLS and Raft paths
   are relative to this JSON file. Omit `pool_id` for a new automatic pool.

```json
{
  "node_id": "00000000-0000-4000-8000-000000000001",
  "listen": "0.0.0.0:9443",
  "database": "raft.sqlite",
  "ca": "pool-ca.pem",
  "cert": "node-a.pem",
  "key": "node-a.key",
  "bootstrap": "auto",
  "peers": {
    "00000000-0000-4000-8000-000000000001": {"address":"a.example.org:9443","cert":"node-a.pem"},
    "00000000-0000-4000-8000-000000000002": {"address":"b.example.org:9443","cert":"node-b.pem"},
    "00000000-0000-4000-8000-000000000003": {"address":"c.example.org:9443","cert":"node-c.pem"}
  }
}
```

5. Start the instances in any order and allow the configured peer port between them.
   A fresh node waits until a majority of the configured certificate-pinned
   peers, including itself, attest over mTLS to the same initial topology and
   pristine Raft state. It then initializes Raft with the entire agreed set,
   including any offline node. An initialized peer follows the existing Raft path.
   Any two of the three can form the pool and elect a leader. A lone node cannot
   form the example pool on its own. Running games continue on their hosts
   during a quorum loss.
6. Check the pool in the instance dashboard. Verify the expected voter count, an
   elected leader and eligible hosts. Connect test players to different home
   instances using the same client build, queue and simulation data. A compatible
   host must advertise enough capacity and meet each ticket's latency budget.

Automatic mode derives a pool UUID from the complete sorted initial set of
node IDs, advertised peer addresses and pinned leaf certificate fingerprints.
Different initial sets therefore do not authenticate as the same pool. All
peers must agree on the exact advertised address strings. An explicitly supplied
`pool_id` in automatic mode must match that derived value. Reopening a Raft
database checks its stored pool and node UUIDs, so changing an automatic-mode
address or leaf cannot silently initialize replacement membership over durable data.

The application relies on OpenRaft 0.9.26's `Raft::initialize` rule that
multiple pristine nodes can initialize concurrently with the same configuration.
OpenRaft's engine writes the initial membership at the default log ID, starts
an election, and refuses initialization once a log or vote exists. Later
elections, quorum and membership changes use Raft's committed state. Discovery
is transient and requires no designated voter.
[OpenRaft 0.9.26 initialization contract](https://docs.rs/openraft/0.9.26/openraft/raft/struct.Raft.html#method.initialize)

Existing boolean configurations remain supported. Use the configured
`pool_id` with `"bootstrap": true` on the designated initial node and
`"bootstrap": false` on the other nodes. Keep existing databases and UUIDs
when upgrading those pools.

Preparing allocations have a 35-second confirmation window. The leader
writes its timestamp into the committed allocation command. The absolute
deadline survives leader changes and restarts, and is displayed by the
native offer endpoints. New confirmations and host readiness cannot advance
a preparing room after the deadline. Expiry releases the preparation and
keeps remaining tickets' queue priority. Previously persisted preparations
without a deadline get one 35-second grace window on their first timed
operation. Existing ready and finished rooms keep their state.

Keep voter and home clocks synchronized. Upgrade voting members together
before serving new timed offers. Older binaries can read the historical
format but do not enforce the new expiry command and timestamp.

To back up or move an instance, stop its process and preserve the instance
configuration, keys and all three databases together, including any live SQLite
WAL files. Only one process should own a given instance identity. Starting with a
new Raft database or a second copy of a live voter changes the recovery problem.
Use the existing surviving majority when recovering a failed member.

When the assigned host restarts with its original identity and journal, it checks
previously reserved games against the rooms still running in that process. For
a committed game whose world was lost, the host journals a terminal
`host_restarted` outcome and submits it to the pool. After quorum commits the
outcome, each home clears the player's room and returns to the menu. This path
awards no win, completion or quest credit. A persisted natural result takes
precedence and uses the normal settlement outbox. A stopped home or an election
alone leaves the running game on its host.

## Configure host latency

An omitted or empty `[federation.latency_ms]` table enables automatic measurement.
A home measures small application round trips over its configured mTLS peer
connections when preparing a ticket. Only current voters are probed. The home
records itself at 0 ms and skips unreachable peers. Concurrent enqueues share
the measurements. A complete set is cached for 20 seconds, and a partial set for
5 seconds. The service sends these probes only when it needs an enqueue snapshot.

This RTT measures the home-to-host service path and can include connection
establishment. Players connect to the game host directly, so their measured
network latency can differ.

Set a nonempty table for an operator-maintained latency allowlist. The service
uses these values as its baseline and considers only the named hosts. Replace
the sample UUIDs with configured voters.

```toml
[federation.latency_ms]
"00000000-0000-4000-8000-000000000001" = 0
"00000000-0000-4000-8000-000000000002" = 35
"00000000-0000-4000-8000-000000000003" = 75
```

To advertise an existing client probe endpoint, configure up to 32 sites on the home. Each entry maps a unique site name and advertised host to a distinct pool node. Android receives just the site, host and port in `startSessionForPlayer.pingHostPortInfo`.

```toml
[[ping_sites]]
site = "WEST"
host = "probe.example.net"
port = 1119
node_id = "00000000-0000-4000-8000-000000000002"
```

Android reports `site`, `host`, `avg`, `stdDev`, `max` and `current` in `joinLobby`. The home accepts only exact configured site/host pairs and finite, nonnegative numeric samples below the observed two-second timeout. For each eligible host it takes the greater of the trusted baseline and the rounded-up largest reported RTT. This also applies to the queueing member's report for a party. Other members retain the baseline estimate and the matcher considers the worst ticket estimate. Unknown sites, missing reports and timeout samples leave the baseline in effect. Client-supplied host strings are compared locally. The server only connects to its configured federation peers.

The isolated 147219 Android client connected nine times to a loopback TCP endpoint per run. An accepting listener saw no TCP payload or UDP datagrams. With the connection held open, the client reported a 2000.520 ms average; immediate orderly TCP closure yielded 2000.105 ms. Both outcomes match a timeout. The expected successful application response remains unverified. Configure an existing compatible service if available.

Each enqueue stores the entire ticket in the home journal before submission.
Timeout retries, cancellation and restart reuse that payload. An upgrade reads
an older committed ticket from the pool and saves its original values. To
evaluate a fresh snapshot, cancel and requeue. A new ticket may still use
measurements within the cache lifetime.

## Change membership

Run `openhalcyon pool --config runtime/server.toml members` on any member. The
CLI tries its configured peers, discovers the elected leader, and reads the
committed voter sets and addresses. A peer with a pinned certificate can also
read `GET /federation/v1/membership` on the peer listener using mTLS and the
`x-halcyon-pool` header.

Before editing an automatically initialized pool's trust list, advertised
addresses or pinned leaf certificates, read and save its current UUID with
the `members` command or dashboard. Set that UUID as explicit `pool_id`
and set `"bootstrap": false` on every member. This preserves the initial
identity while Raft manages subsequent membership. Keep the original
databases, then apply coordinated trust and topology changes. A node started
against an existing pool should use this explicit non-bootstrap configuration.

To admit a new operator, distribute its public certificate and address to
the existing members' `peers` maps, then roll the instances one at a time.
Start the new node with the same pool identity, the complete trust list and
`"bootstrap": false`. A voting operator can submit the expected old voter set
and the desired set as comma-separated UUIDs:

```sh
openhalcyon pool --config runtime/server.toml set-voters --expect "$OLD_VOTERS" --voters "$NEW_VOTERS"
```

The leader first catches up new learners, then commits the membership change
through Raft. Use the same command to remove a voter. Changes compare the
expected membership against committed state. If leadership changes during a
request, read the membership again to determine the outcome before retrying.

The integration test starts three independently authenticated TLS peers, exercises
elections, and the server test drives cross-instance admission through CE calls.

```sh
cargo test --locked -p halcyon-federation
cargo test --locked -p openhalcyon --test peer_play
```

## State storage and upgrade

Each node commits newly applied entries to a SQLite journal before updating its in-memory pool, request outcomes and applied index. A background worker encodes each full CBOR snapshot. The transaction that publishes the snapshot also removes journal entries through its applied index. Entries that arrived during snapshot generation survive this transaction and replay in order at restart. OpenRaft can purge its consensus log independently. Snapshots retain settled matches, rejected requests and idempotency outcomes.

Migration preserves the latest applied state from the older database, including changes since its most recently published snapshot. Peers continue to exchange the same CBOR snapshot format. **Back up each node before upgrading it.** Use the SQLite backup API or stop the node to make a consistent file backup, including any remaining WAL. To roll back to an older binary, stop the node and restore its pre-upgrade backup. The old binary refuses to read a migrated database.

Run the release benchmark on the storage device you intend to use.

```sh
cargo test --release -p halcyon-federation store_write_amplification -- --ignored --nocapture
```

The benchmark seeds disposable databases with 128 or 1,024 settled matches. Each match contributes five committed requests for enqueue, allocation, acceptance, host readiness and settlement. It then submits the same 32 new requests individually through the previous full-state CBOR implementation and the incremental implementation. Both use SQLite WAL with FULL synchronization. The test truncates the WAL before measurement, temporarily disables automatic checkpointing, and records the growth of the actual WAL file. Incremental snapshot time is measured separately. The measurement covers state-machine application on one node and excludes consensus traffic, Raft log appends and elections.

| Settled matches / previous requests | Previous apply time | Incremental apply time | Previous WAL growth | Incremental WAL growth | Full snapshot |
|---|---:|---:|---:|---:|---:|
| 128 / 640 | 219.0 ms | 67.1 ms | 5,932,832 B | 148,352 B | 4.09 ms |
| 1,024 / 5,120 | 1,324.4 ms | 65.1 ms | 44,833,872 B | 148,352 B | 19.41 ms |

These Windows 11 measurements on a Ryzen 9 9950X3D show constant WAL growth for the 32 new requests across the two history sizes. The cost of a complete snapshot grows with the retained history.

### Cross-home match chat

The player API and the CE chat WebSocket accept messages from a seated player on
their authenticated home. The sender home finds the participating homes in the
committed allocation and saves one pending delivery per home before returning.
Only those homes receive the text over mutually authenticated peer connections.
Raft holds the roster. Chat text travels directly between homes.

Each receiver verifies the sender's home and roster, then inserts an event for
every active local participant in one SQLite transaction with the message receipt.
An acknowledgement lost after commit can be retried with the same message ID.
Clients observe the usual home event cursor. Delivery can lag while a peer catches
up with the committed roster. The sender retries for up to two minutes while the
room is active and admits at most 4,096 outstanding home deliveries per node.
