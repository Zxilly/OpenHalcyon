# Operating a matchmaking pool

Each operator runs the same server executable and keeps a separate instance UUID,
platform database, federation journal, Raft database and private keys. Players sign
in to their own instance. The elected Raft leader orders ticket and room decisions.
Any voter can accept local players, submit a proposal, win the next election or host
a match. Movement and combat stay on the room host.

## Configure three instances

1. Run `openhalcyon init` in an empty instance directory on each machine. Use each
   machine's reachable public hostname. Keep the three generated `instance_id` values.
2. Agree on a pool UUID, a voting membership and a pool CA. Issue each node a distinct
   TLS leaf certificate covering the DNS name or IP in its advertised peer address.
   If the leaf includes extended key usages, include both client and server authentication.
   Each operator keeps its own leaf private key. Exchange the CA and public leaf
   certificates so that every member can pin the same node identities.
3. Add the following section to each instance's `server.toml`. The UUIDs below
   represent three separate instances. Replace them with the IDs from step one.
   The example latencies are in milliseconds. Configure values for each home
   instance according to its latency to the candidate hosts.

```toml
[federation]
cluster_file = "pool.json"
journal = "peer-play.sqlite"
max_latency_ms = 120

[federation.latency_ms]
"00000000-0000-4000-8000-000000000001" = 5
"00000000-0000-4000-8000-000000000002" = 35
"00000000-0000-4000-8000-000000000003" = 75
```

4. Write `pool.json` on each node. The example below belongs to node A. On nodes B
   and C, select their own `node_id`, `cert` and `key`, and set `bootstrap` to
   `false`. The full `peers` map and pool UUID are identical on all three nodes.
   The TLS and Raft paths are relative to this JSON file.

```json
{
  "pool_id": "aaaaaaaa-aaaa-4000-8000-aaaaaaaaaaaa",
  "node_id": "00000000-0000-4000-8000-000000000001",
  "listen": "0.0.0.0:9443",
  "database": "raft.sqlite",
  "ca": "pool-ca.pem",
  "cert": "node-a.pem",
  "key": "node-a.key",
  "bootstrap": true,
  "peers": {
    "00000000-0000-4000-8000-000000000001": {"address":"a.example.org:9443","cert":"node-a.pem"},
    "00000000-0000-4000-8000-000000000002": {"address":"b.example.org:9443","cert":"node-b.pem"},
    "00000000-0000-4000-8000-000000000003": {"address":"c.example.org:9443","cert":"node-c.pem"}
  }
}
```

5. Start the three instances and allow the configured peer port between them. The
   bootstrap flag initializes the first durable membership on a fresh database.
   Subsequent starts load the committed membership. The first election determines
   leadership. A three-voter pool needs two reachable voters to commit or perform
   a linearizable read. Running games continue on their hosts during a quorum loss.
6. Check the pool in the instance dashboard. Verify the expected voter count, an
   elected leader and eligible hosts. Connect test players to different home
   instances using the same client build, queue and simulation data. A compatible
   host must advertise enough capacity and meet each ticket's latency budget.

To back up or move an instance, stop its process and preserve the instance
configuration, keys and all three databases together, including any live SQLite
WAL files. Only one process should own a given instance identity. Starting with a
new Raft database or a second copy of a live voter changes the recovery problem.
Use the existing surviving majority when recovering a failed member.

## Change membership

Run `openhalcyon pool --config runtime/server.toml members` on any member. The
CLI tries its configured peers, discovers the elected leader, and reads the
committed voter sets and addresses. A peer with a pinned certificate can also
read `GET /federation/v1/membership` on the peer listener using mTLS and the
`x-halcyon-pool` header.

To admit a new operator, distribute its public certificate and address to
the existing members' `peers` maps, then roll the instances one at a time.
Start the new node with the same pool identity, the complete trust list and
`bootstrap = false`. A voting operator can submit the expected old voter set
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
