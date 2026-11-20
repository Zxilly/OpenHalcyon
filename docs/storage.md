# SQLite storage migration

The platform identity path uses the SQLite backend of Diesel 2.3.4 with embedded migrations. Diesel and the remaining SQLite components link against the same bundled libsqlite3-sys 0.35 dependency. The server build uses its existing native compiler toolchain.

`halcyon-storage` owns the typed account, player and session schema used by this path. It adopts the existing `local_accounts` table through an idempotent migration and records the applied revision in `__diesel_schema_migrations`. On a fresh database the platform initializes the shared identity tables before opening the account store. On an existing database the migration leaves the account table, recovery keys, session keys and password hashes in place. Reverting the history marker preserves the shared table and its data.

Account lookup uses a typed Diesel query. Account linking validates the session and inserts the unique login/owner pair in one immediate SQLite transaction. Password verification stays outside SQLite's write transaction. Afterwards, the transaction verifies that the password record still matches and rotates recovery and session digests atomically. Guest creation and recovery use the same session-issuance helper as password login. Creation inserts the player and session together. Recovery keeps the existing recovery digest, and each successful recovery replaces the previous session while marking the player online. Rename and logout use the same ORM connection. A repeated logout removes no row and sends no change notification.

Composite lobby profiles and matchmaking code continue to use the existing connection to the same underlying database. Their authorization checks stay in their own transactions until the relevant lobby tables migrate together. There is one authoritative copy of each identity table.

The storage connection enables foreign keys and a five-second busy timeout. The platform owns its WAL setup. Schema adoption runs under an immediate transaction so concurrent startups contend at the writer lock before deciding which migration to run. Two private in-memory platform instances use different shared-memory URIs. Within an instance the legacy and ORM connections open the same URI.

Build and test with the repository's normal bundled SQLite toolchain.

```powershell
cargo test --locked -p halcyon-platform --test accounts
cargo tree -i libsqlite3-sys
```
