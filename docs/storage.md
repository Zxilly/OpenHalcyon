# SQLite storage migration

The platform identity path uses the SQLite backend of Diesel 2.3.4 with embedded migrations. Diesel and the remaining SQLite components link against the same bundled libsqlite3-sys 0.35 dependency. The server build uses its existing native compiler toolchain.

`halcyon-storage` owns the typed account, player and session schema used by this path. It adopts the existing `local_accounts` table through an idempotent migration and records the applied revision in `__diesel_schema_migrations`. On a fresh database the platform initializes the shared identity tables before opening the account store. On an existing database the migration leaves the account table, recovery keys, session keys and password hashes in place. Reverting the history marker preserves the shared table and its data.

Account lookup uses a typed Diesel query. Account linking validates the session and inserts the unique login/owner pair in one immediate SQLite transaction. Password verification stays outside SQLite's write transaction. Afterwards, the transaction verifies that the password record still matches and rotates recovery and session digests atomically. Guest creation and recovery use the same session-issuance helper as password login. Creation inserts the player and session together. Recovery keeps the existing recovery digest, and each successful recovery replaces the previous session while marking the player online. Rename and logout use the same ORM connection. A repeated logout removes no row and sends no change notification.

Recipient-local notification reads authorize the session and select its cursor and event on one Diesel read snapshot. Existing SQLite triggers assign recipient-local ordinals and the legacy backfill remains in place. Native post-match acknowledgement uses an immediate Diesel transaction for its session check, terminal-state check, seat release and notification. An active queue or match retains its seat.

Local match settlement (`finish_match`, `abort_match`) and federation projection and release (`federated_project`, `federated_acknowledge`, `federated_release`) also run in immediate Diesel transactions on `AccountStore` without holding the legacy connection mutex simultaneously. They require no additional database migration and operate directly against the current platform schema, updating `matches`, `federated_queue` and `federated_queue_members` in place rather than replacing rows so `ON DELETE CASCADE` children stay intact and SQLite settlement, notification and match-statistics triggers (`native_local_settlement`, `native_peer_settlement`, `event_notifications`, `ce_snapshot_match_handle`, `ce_result_event`) fire identically, waking notification readers explicitly after commit whenever rows change.

Composite lobby profiles and matchmaking code continue to use the existing connection to the same underlying database. Their authorization checks stay in their own transactions until the relevant lobby tables migrate together. There is one authoritative copy of each identity table.

The community crate uses typed Diesel queries for friends, presence, parties, guilds, equipment, inventory, quests and redemptions. Native party mutations share this implementation. Membership changes, invalidation of old invites and insertion into the platform notification outbox commit in one transaction. CE provider attestations and in-app purchase receipts (`ce_provider_identity` and `ce_provider_orders`) now share the community Diesel connection and initialize their latest SQLite table schemas idempotently inside an immediate transaction without a Diesel migration: Ed25519 JWT signature verification and SHA-256 receipt digest computation remain outside database locks, verified identity expiry reads run in deferred Diesel read transactions, and order preparation, monotonic identity expiry updates, and IAP fulfillment (`Community::award_item_in_orm_transaction` paired with receipt recording on `ce_provider_orders`) commit or roll back atomically inside immediate Diesel transactions. The matchmaking and spectator services likewise keep their party locks and authorization reads inside the transactions that consume them.

The storage connection enables foreign keys and a five-second busy timeout. The platform owns its WAL setup. Schema adoption runs under an immediate transaction so concurrent startups contend at the writer lock before deciding which migration to run. Two private in-memory platform instances use different shared-memory URIs. Within an instance the legacy and ORM connections open the same URI.

The peer-play crate owns a separate Diesel journal for hosted matches, pending results and cross-instance chat delivery. The migration adopts the existing tables and delivery indexes. Allocating the next sender sequence and inserting the intended recipients run under one immediate transaction. Acknowledgements and recovery use the same journal after restart.

Each federation node uses Diesel for its OpenRaft log, vote, commit position, applied state and snapshots. It retains the SQLite tables and CBOR encoding so a node can reopen its existing state. Snapshot installation validates the membership and updates the persisted state atomically. A failed transaction leaves the in-memory machine at its previous applied position.

Build and test with the repository's normal bundled SQLite toolchain.

```powershell
cargo test --locked -p halcyon-platform --test accounts
cargo test --locked -p halcyon-community -p halcyon-ce-platform
cargo test --locked -p halcyon-peer-play --lib journal
cargo test --locked -p halcyon-federation
cargo tree -i libsqlite3-sys
```
