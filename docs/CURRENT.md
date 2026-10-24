# Current status

2026-10-25: aligned runtime extraction with the sibling project's Rust-source
convention. Supported-build identity, addresses and owned defaults now live in
`src/client_build.rs`. Removed the separate public extraction-config folder,
generic JSON recipe interpreter and duplicate Python extractor. Actual client
strings still come exclusively from the user's executable at runtime.

The server remains stopped at the user's request. Existing local settings,
custom profile, database, logs and backups are preserved. No restart, console
probe, DNS/router edit, commit or publication occurred.

## Setup and validation

New installations use `executable` in TOML; existing custom installations can
retain `profile`. Exactly one source is required. The Rust ELF parser and input
fingerprint gate remain. All offline checks and DNS/profile/bundle exports use
the Rust binary. No separate extraction JSON or generated profile is needed to
build or start executable mode. Public test fixtures and private profile/endpoint
exports can still use JSON; these are different from an extraction recipe.

The public guide identifies the input using the Rust `SUPPORTED_BUILD` definition.
All actual client inputs, derived identifiers and exports stay under ignored
`game/`. Public docs and shared agent skills remain part of the source release.
The package/binary remains `bo2-server`, with local `bo2-server.toml` settings.
Client IP enrollment is not required; protocol/token/capacity checks still apply.

## Preserved behavior

The local data profile produces a startup bundle byte-identical to the last
tested deployment (9,950 bytes). League IDs/rules, matchmaking region0, two-player
minimum, long-running season, keep-alive replies and wins-only persistence remain
unchanged. The runtime database has not been modified by this preparation.

Before this refactor, a two-console match yielded a saved four-to-five win and a
user-confirmed five-win achievement. The initial four wins were operator-seeded.
The public suite separately verifies a new player progressing from zero to five,
with duplicate/stale reports, ranked reads and database reopen between matches.
The user reports that the updated build works with the preserved custom profile.
This does not establish a new completed-match/result test or console acceptance
for executable-mode defaults; byte equality and offline checks remain separate.

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


## Rust-source convention verification

All102 Rust tests,5 publication-gate tests, formatting, strict Clippy and the
locked offline release build passed. The publication scan/export covers76 files
with no separate config directory or extraction helper. Tracked working-tree
deletions are excluded from the source export without staging; tracked private
files still fail the audit. This does not rewrite or audit away Git history.

Real-input profile and endpoint exports are byte-identical to the previous
runtime loader. The default9,794-byte and customized9,950-byte bundles are each
unchanged. A fresh source-only export built and passed check/profile/endpoint/
bundle exports from another working directory using only the user's executable.
No database or listener was created. No new console acceptance is claimed.
