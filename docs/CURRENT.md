# Current status

2026-10-24: publication preparation. The live server was stopped at the user's
request and remains stopped. No commit, push, network configuration change or
public deployment was performed.

The original project folder remains authoritative. Final checks from that folder
passed: 96 Rust tests, 4 publication-gate tests, formatting, strict Clippy and the
release build. The publication scan covered 76 files with zero failures.

The package/binary is now `bo2-server`, as requested by the user. Server settings
use `bo2-server.toml`, with a public `bo2-server.example.toml`; the local settings
were migrated with the same values and database path. Client profiles remain JSON.
Apart from the project name, public source, docs and fixtures use generic
terminology. Actual client strings, old research documentation, local
input identities and user-supplied executable files are retained under ignored
`game/`. Local settings and runtime state are excluded from Git.

Client IP filtering has been removed at the user's request. Any source address
that can reach the configured listener may connect. Local/example settings no
longer contain `allow_ips`; handshake validation, source-bound tokens and capacity
limits remain in effect. A two-client loopback regression passes without any
client enrollment configuration. The existing bind address is unchanged.

## Fresh setup generation

The agent guide now uses `scripts/prepare_game.py` and the reviewed public
`config/client-layout.json`. With the exact user-owned decrypted executable, it
generates a local input copy, runtime profile, application endpoint file and hash
manifest. No private research folder is needed. Unknown digests and conflicting
existing files are rejected. Generic display labels are the only policy-text
differences from the existing profile; all category keys, gameplay tokens, IDs
and rules are preserved. See [preparation](PREPARATION.md) for the full workflow.
Four synthetic preparation tests complement the four publication-gate tests.
A fresh source-only setup is checked offline; this does not certify the generated
default display labels on consoles. The existing local profile is preserved.

## Preserved behavior

The local data profile produces a startup bundle byte-identical to the last
tested deployment (9,950 bytes). League IDs/rules, matchmaking region0, two-player
minimum, long-running season, keep-alive replies and wins-only persistence remain
unchanged. The runtime database has not been modified by this preparation.

Before this refactor, a two-console match yielded a saved four-to-five win and a
user-confirmed five-win achievement. The initial four wins were operator-seeded.
The public suite separately verifies a new player progressing from zero to five,
with duplicate/stale reports, ranked reads and database reopen between matches.
The renamed/profile-driven build requires a future authorized console retest;
byte equality and loopback tests do not substitute for that acceptance.

## Verification and remaining work

See [publication review](PUBLICATION_REVIEW.md) for the final check results and
export boundary. See [agent guide](../AGENTS.md) for setup, [profile](PROFILE.md)
for local input requirements and [protocol](PROTOCOL.md) for service contracts.

Independent completed-match verification, broader Internet connectivity, host
migration and complete optional-service support remain unfinished. All players
share one geographic group and division; geographic matchmaking is deferred.
The exact achievement predicate is not independently traced. The observed
achievement succeeded with other League counters still at their defaults.

No distribution license has been selected. Retained package and dependency names
are required tooling/attribution, not embedded client branding. Do not publish
private inputs, log contents, real identities or the whole working directory.
