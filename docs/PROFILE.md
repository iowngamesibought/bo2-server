# Local client profile

`game/profile.json` is required for deployment and ignored by Git. Keep actual
map/script/member names there. The public `tests/fixtures/profile.json` describes
the shape using invented data only; do not deploy it to a retail client.

Generate the supported default profile with `scripts/prepare_game.py` and the
user's already decrypted executable, following [preparation](PREPARATION.md).
The public layout contains file ranges and a full-input digest; extracted tokens
are written only to the ignored output. Existing profiles are never overwritten.

| Field | Meaning |
| --- | --- |
| `schema_version` | Must be1. |
| `notice` | Local provenance note; never sent to the client. |
| `bundle_prefix`, `bundle_suffix` | Match the requested publisher filename; all matched languages receive the same generated bundle. |
| `playlist_member_name`, `league_member_name` | Exact names the client searches inside the bundle, at most31 bytes each. |
| `league_category_filter` | Client menu's category filter for competition playlists. |
| `minimum_player_settings` | Client setting tokens used to set the two-player minimum. |
| `maps` | Map tokens supported by every enrolled client installation. |
| `leagues` | IDs, display names/descriptions, party rules, placement count, playlist ID, category token and mode/script tokens. |
| `categories` | Public category key, name and description. |
| `playlists` | Public playlist ID, name, description, category, maximum party size and mode/script tokens. |

League fields are `id`, `name`, `description`, `team_size`, `teams_allowed`,
`randoms_allowed`, `placement_games`, `playlist_id`, `category`, `gametypes`.
Category fields are `key`, `name`, `description`. Public playlist fields are
`id`, `name`, `description`, `category`, `max_party`, `gametypes`.

Identifiers must contain only ASCII letters, digits, underscore, hyphen or dot.
Labels may not contain control characters, quotes or backslashes. IDs must be
unique in their namespace. Playlist IDs are0..199; party sizes are1..18. Public
playlists must reference declared public categories. Collection limits and the
client's64KiB startup buffer are enforced before opening the listener.

The renderer generates competition JSON and playlist text from this one profile.
It puts all category sections before playlist sections and uses CSV rotation
rows. The same League IDs/rules seed SQLite, avoiding a second mismatched source.
Season1 currently runs2020-01-01 through2038-01-19 03:14 UTC. Published seasons
are checked for consistency on startup; an incompatible existing database fails
rather than resetting progress. Do not edit IDs or immutable rules casually.

The profile changes names and policy, not the transport codec or arbitrary
client binary layouts. Another build needs its own evidence and compatibility
check. Generated startup bundles, extracted identifiers and prior deployment
profiles stay under `game/`, even though some are plain text.
