# Engineering conventions

## Protocol contracts

- Define each REST or RPC envelope, event and response with a named Serde type.
  Serialize that type at the boundary. Use tuples for positional RPC parameters.
  The workspace forbids `serde_json::json!` in library code, binaries, examples
  and tests. Do not replace it with a JSON builder macro, inline JSON text or a
  string-keyed map for a fixed schema.
- Model closed protocol vocabularies with enums in the crate that owns the
  contract. This includes RPC methods, opcodes, state transitions, event
  variants, result codes and known action selectors. Define the wire spelling
  or numeric discriminant at the enum boundary and use its variants in callers.
  Preserve unknown opcodes and extension data in lossless representations.
- Give fixed wire constants and offsets descriptive names. Quantities, indices,
  user input and original sample observations retain their actual data types.
  Use a map for genuinely variable operator or player keys.
- Where compatibility requires `serde_json::Value`, convert the typed payload
  at that boundary. Verify serialized field names, null versus omission, JSON
  numeric types and binary widths against an independent wire fixture.

## Validation and commits

- Run workspace format, Clippy and tests with the checked-in lockfile. Include
  all targets so test and example code follows the same conventions.
- Before each commit, inspect the recent messages with `git log --oneline -20`
  and follow the repository's prevailing Conventional Commit style.
- Keep the workspace version fixed until the owner explicitly requests a release.
