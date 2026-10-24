# Implemented protocol boundaries

Application frames have a four-byte little-endian length. Authentication and
login use a bit-packed schema; lobby tasks use typed byte fields. Protected
client payloads use a truncated HMAC plus CBC framing; the server direction has
a distinct clear-body marker. Public vectors contain synthetic values only.
The platform ticket signature is not verified by the experimental bootstrap.

| Service | Scope |
| --- | --- |
| `0x51` | Team lookup/membership, subdivision enrollment and season views. |
| `0x52` | Statistics snapshots and ranked reads; only wins are applied. |
| `0x15` | In-memory advertised sessions and filtered candidate lookup. |
| `0x0a` | Profile-driven startup bundle, optional patch decline and persistent user blobs. |
| `0x08`, `0x1c`, `0x44` | Profile, group membership and presence paths. |
| `0x0c`, `0x1b` | Time and shared private geographic metadata. |
| Other implemented modules | Bounded counters, archive and leaderboard adapters; see tests for supported operations. |

## Connection lifetime

The inspected client sends an empty frame after40s without sending and closes
after120s without receiving. Reply with exactly four zero bytes, outside task
FIFO, message seeds and task limits. Complete empty frames renew the server idle
wait but not the absolute session limit. Nonempty bodies remain size/time bounded.
The raw measurement initialization path returns its traced failure format until
the actual measurement exchange is implemented; it must not send truncated success.

## Wins and results

The client's winner/loser processing prepares a statistics snapshot. Uploads do
not carry a reliable unique match identifier or a direct authoritative win event.
The server checks team membership, subdivision and open season; records every
parsed upload; and atomically applies only `uploaded == stored+1`. Equal counts
are no-ops. Decreases, jumps and invalid context are recorded without applying.
A success acknowledgement does not by itself mean a win was added.

New solo teams start at0; ranked reads return persisted wins. Other League
fields retain defaults. Wins are scoped to team, League and season, not a global
cross-series account total. Tests cover five successive wins, repeat/stale
snapshots, independent new players and reopening the database between matches.
The client is still trusted to report an earned win; independent outcome
corroboration and server-admitted match identity remain TODO.

## Matching region

Four geographic metadata levels currently share value0 for all players. This
survives the client's advertisement conversion and candidate comparison. Unknown
values previously converted differently on the local/remote sides and caused
candidate rejection. Geography is independent of division/subdivision and wins.
Future geographic policy must preserve consistent IDs in startup data and host
advertisements, define unknown-location fallback, and refresh client caches.
It is deferred until the private population justifies separate regions.
