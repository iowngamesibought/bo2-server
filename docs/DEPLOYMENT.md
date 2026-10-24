# Deployment and recovery

Server settings use `bo2-server.toml`, following the sibling server's example/local
configuration convention. Copy `bo2-server.example.toml` and edit its values.
Relative paths resolve from the configuration directory. The client data profile
remains JSON. Legacy JSON server settings must be converted to TOML; simply
changing the filename is insufficient. Preserve the database path and all values
when migrating. TOML strings need forward slashes or single-quoted literal paths
for Windows paths with backslashes.

Follow the ordered setup in [AGENTS.md](../AGENTS.md). The guide and two shared
skills are included in this repository; no sibling research workspace is needed
for the generic procedure. Client-specific evidence remains local.

The server accepts connections from any source IP that can reach its configured
listener. There is no client IP allowlist or enrollment step. Remove `allow_ips`
from older settings; unknown configuration fields are rejected. Handshake token
validation, token source binding and connection limits still apply.

## Files and checks

Follow [preparation](PREPARATION.md) to place the verified executable under ignored
`game/inputs/`. Configure `executable` for runtime extraction, or retain `profile`
for an existing customized JSON profile; setting both or neither is an error.
Copy the public example only when settings do not exist, select a private bind
address and persistent SQLite path, and explicitly enable the experimental bootstrap.
Paths resolve from the configuration file, not the shell's working directory.

`--check-config` validates the selected source and renders the bundle without a
listener or database. `--write-bundle game/startup.bundle` exports an inspection
copy. Executable mode also supports `--write-endpoints game/endpoints.json` and
`--write-profile game/profile.json`. Export destinations must be new files.
Missing or unsupported input is a startup error, with no synthetic fallback.
These operations do not establish console acceptance or database compatibility.

Run `bo2-server --config bo2-server.toml` after deployment is authorized.
The startup line is `server.listening`. Keep background process identity, binary
hash and log paths in a private runtime record. Do not log raw platform tickets.

## Backup and restore

Use SQLite's backup API for a consistent snapshot; copying only the main file
while a connection has uncheckpointed writes is unsafe. For replacement, stop the
verified owned process first, take/verify a backup, preserve the old binary, then
start the new binary with the same database and bind address. To restore, stop the
server and use a verified consistent backup. Keep rollback files private.

Player names, uploads, wins and season/team state persist. Active session
advertisements, group membership and presence live in memory and disappear on
restart. New enrollments start at zero wins. Existing wins are not reseeded.

## What to observe

| Observation | Interpretation / next check |
| --- | --- |
| No authentication arrives | Check the client's resolver, exact application host override, proxy exemptions and firewall. Do not redirect platform sign-in. |
| Unknown/expired token | Check authentication-to-lobby correlation and source address; do not fabricate account authentication. |
| Match candidates never join | Check candidate field/region consistency before assuming a LAN problem. Region IDs are separate from League divisions. |
| Quiet matches disconnect near two minutes | Verify four-zero-byte keep-alive replies and connection-specific read-error logs. |
| Result reply is successful but wins unchanged | Inspect the receipt: equal wins is a legitimate no-op; other rejected snapshots are also receipted. |
| Database wins differ from a cached client | Reload the client's League context before testing; do not increment on every upload. |
| Unserved optional request | Preserve the service/op and error; one successful match does not certify that service. |

Keep-alives renew the300-second idle wait. A six-hour absolute session limit is
still enforced. Internet NAT traversal, server-authoritative match outcomes and
host migration are not fully validated. Do not use active console probes during
a review; user-driven gameplay tests are separate acceptance steps.
