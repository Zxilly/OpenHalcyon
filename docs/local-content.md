# Local game content

An optional `content_file` at the top level of `server.toml` loads a JSON catalog
when the instance starts. Item identifiers refer to the same inventory used by
equipment, quests, forging and the local shop. The file path is relative to the
server configuration. The following example creates a one-time practice chest
and a recipe that spends seven units of its reward.

```json
{
  "items": [
    {"id": "silver", "slot": "currency", "name": "Silver"},
    {"id": "practice_card", "slot": "card", "name": "Practice card"}
  ],
  "exchanges": {
    "collectHeroChest": {
      "practice_chest": {"reward": {"silver": 10}, "limit": 1}
    },
    "forgeCard": {
      "practice_card": {"cost": {"silver": 7}, "reward": {"practice_card": 1}}
    }
  }
}
```

Every recipe belongs to one RPC method and one SKU. A positive lifetime limit
is required for grants that cost nothing. The server debits the configured cost,
credits the reward, updates depleted equipment and records the receipt in one
transaction. An exact retry with the same session token reuses the receipt.
Clients receive a fresh session token after successful RPCs. Price and inventory
come from the server. Operators can configure a receipt verifier and an inventory
mapping. An unconfigured instance reports that the provider is unavailable.
[Operator attestations](provider-attestations.md)

An optional `quests` array accepts definitions with `id`, `category`, `title`,
`goal`, `skippable` and an optional `reward` pair such as `["silver", 2]`.
The `complete_match` category advances when the player's home records a completed
game. The `win_match` category advances for the winning seats. Retrying the same
settlement credits each player once, including games hosted on a pool peer. An
optional `views` object maps supported manifest or offer RPC names to public
response fields. Use original content or data supplied by the operator.
