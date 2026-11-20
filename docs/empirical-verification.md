# Reproducing the empirical opcode comparison

Build the two offline inventories and pass the verifier the three original ARAL session directories used to produce the supplement, the Android Solo VGR chunk directory, and the extracted reference directory. The checker prints one aggregate row per opcode and returns a failure on disagreement.

```sh
cargo build -p halcyon-protocol --example capture_inventory --example replay_inventory
python tools/verify_protocol_empirical.py --bin target/debug/examples \
  --vgr <vgr-directory> --empirical <supplement>/protocol_empirical \
  <first-aral-session> <second-aral-session> <third-aral-session>
```

* **TCP.** The inventories independently reassemble fragmented streams, validate both plaintext handshakes, decrypt each subsequent frame, verify its encrypted round trip, and dispatch its typed fields. Sizes exclude the outer two-byte TCP length and include the opcode and any encrypted alignment bytes.
* **VGR.** The reader decodes and reserializes every original record. Its measured message size includes the opcode and excludes the surrounding replay record envelope. The checker compares the ten first hero declarations by actor, template, skin hash, team and message length using the Rust spawn reader.
* **Comparison.** For every opcode the checker verifies the original size-frequency pairs, TCP direction set, separate TCP and VGR counts, enum registration, and Android reader classification. The extractor keeps at most eight sizes per opcode. The checker resolves equal-frequency ties at the cutoff by accepting any original size with the tied frequency. It prints the full measured histogram.

The three original sessions contain 176,191 encrypted records plus three plaintext hellos and three plaintext acknowledgements. The Android Solo replay contributes 43,160 records. All 105 observed opcode classes matched. Each of the 219,351 post-handshake messages dispatched to a typed reader, and all ten first hero declarations matched the empirical entity table. The check covers the registered prefixes and typed readers exercised by this corpus. Unnamed operands and preserved suffixes still require separate evidence before implementing gameplay effects.

The Android registry lists a fixed size for 84 reference rows. Twenty have an ignored or clock reader, and the outgoing player identity has no Android read-side row. The three-byte plaintext acknowledgement does not have the same size as the Android heartbeat reader's ten-byte block. It is verified against the handshake byte contract. The supplement's hello note labels the last four bytes as the address length. In all nine original TCP streams that word equaled the recorded endpoint port and differed from the address length. The checker compares it to the selected captures' private metadata without printing it. The public hello endpoint reader exposes the full word and a checked 16-bit port.
