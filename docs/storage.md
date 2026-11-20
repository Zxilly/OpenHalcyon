# SQLite storage migration

The password-account path uses the SQLite backend of Diesel 2.3.4 with embedded migrations. Diesel and the remaining SQLite components link against the same bundled libsqlite3-sys 0.35 dependency. The server build uses its existing native compiler toolchain.

`halcyon-storage` owns the typed account, player and session schema used by this path. It adopts the existing `local_accounts` table through an idempotent migration and records the applied revision in `__diesel_schema_migrations`. On a fresh database the platform initializes the shared identity tables before opening the account store. On an existing database the migration leaves the account table, recovery keys, session keys and password hashes in place. Reverting the history marker preserves the shared table and its data.

Account lookup uses a typed Diesel query. Account linking validates the session and inserts the unique login/owner pair in one immediate SQLite transaction. Password verification stays outside SQLite's write transaction. Afterwards, the transaction verifies that the password record still matches and rotates recovery and session digests atomically. Guest and matchmaking code continues to use its existing connection to the same underlying database. There is one authoritative account table.

The storage connection enables foreign keys and a five-second busy timeout. The platform owns its WAL setup. Schema adoption runs under an immediate transaction so concurrent startups contend at the writer lock before deciding which migration to run. Two private in-memory platform instances use different shared-memory URIs. Within an instance the legacy and ORM connections open the same URI.

Build and test with the repository's normal bundled SQLite toolchain.

```powershell
cargo test --locked -p halcyon-platform --test accounts
cargo tree -i libsqlite3-sys
```
