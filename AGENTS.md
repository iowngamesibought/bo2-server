# Agent guide for bo2-server

Help the user deploy this server for their own client installation. Read
[README.md](README.md), then [docs/CURRENT.md](docs/CURRENT.md). Only load the
instructions and evidence needed for the current step. User instructions and
existing authorization take precedence over this guide.

<!-- Shared ground rules: copied verbatim from the game-server agent-docs template. Edit them there, then copy to every project. -->
## Ground rules

- **Use only the user's own copy of the game.** Never download game files, decrypted executables, keys or license files from the internet, and never suggest where to get them. If the user doesn't have the files, stop and tell them what's missing.
- **Decrypting the executable is the user's job.** Where the server needs the executable decrypted, the user provides that form from their own copy; it is their responsibility. Don't explain how to decrypt it, and never obtain a decrypted copy elsewhere.
- **Never put game-derived files in this repository**: executables, archives, generated output, decompiler projects or exports, server logs, stored uploads. Keep them in a folder outside it, or in the git-ignored folders this guide names.
- **The host names you find go into the user's DNS setup only.** Don't write them into this repository (code, docs, config examples, commits or issues), and don't copy anything else out of the executable: no keys, code or text. The server reads what it needs from the user's files at runtime.
- **Change the user's system only with their agreement**: firewall rules, DNS, router settings.
<!-- End of shared ground rules. -->

In these rules, the repository exclusion applies to the published source tree;
local copies and generated identifiers may be kept in the explicitly ignored
`game/` folder. Never include that folder in a release archive.

Private inputs and research belong in ignored `game/`. Local deployment settings
belong in ignored `bo2-server.toml`; logs and databases belong in ignored
`runtime/` or an external directory. Never publish these folders or settings.
`CLAUDE.md` imports this guide. The shared skills are also mirrored under
`.agents/skills/` for agents that discover that location.

## 1. Generate the local game folder

Use the [pick-game-files](.claude/skills/pick-game-files/SKILL.md) skill to select
**the already decrypted multiplayer executable from the user's installed update**.
Reuse an already identified local file. The supported input is a 64-bit big-endian
executable; its exact size and SHA-256 are in `config/client-layout.json`.
Do not accept a renamed encrypted file, a base-game executable overridden by an
update, or a different build merely because its filename looks right.

For a fresh installation, install no extra analysis tools. Python 3.11 or newer
and the public source tree are enough to generate the private files:

```text
python scripts/prepare_game.py --executable "<absolute path to user's decrypted executable>"
```

The helper verifies the input against the reviewed layout before writing files.
It reads only the mapped identifier ranges, assembles composite settings/category keys
from mapped native fragments, and applies the owned competition policy. It does
not download, decrypt, execute the client, change DNS or open a server listener.
The layout ships offsets/lengths and a digest, not the extracted client strings.
Generated display labels are generic operator policy; established category keys
are preserved.
All extracted strings and the copied input stay in ignored `game/`.

Successful preparation produces exactly these files:

| File | Purpose |
| --- | --- |
| `game/inputs/client.elf` | Verified copy of the supplied executable; provenance/recovery input. |
| `game/profile.json` | Required runtime profile: bundle names, map/mode/settings tokens, series and playlist policy. |
| `game/endpoints.json` | The two application DNS names, TCP port and roles for step 3; never a broad provider suffix. |
| `game/setup-manifest.json` | Input/layout digests and output hashes; keep with the generated files. |

Only `profile.json` is read by the running server. No encrypted executable,
package metadata, private research tree, sibling repository or prebuilt bundle
is required for this supported preparation path. The runtime builds its startup
bundle from the profile. Step 2 exports a copy for verification.

Repeated preparation with identical inputs is a no-op. A conflicting existing
file is an error: never overwrite a working profile to make the helper pass.
For takeover, validate the existing profile first. If only provenance or endpoint
files are missing, prepare into `game/setup-review/`, compare the profile using
[the preparation guide](docs/PREPARATION.md), and use the endpoint/manifest files
from that directory after verifying the same input digest. Preserve existing operator labels,
IDs, rules, database path and gameplay settings.

An unsupported digest stops before generation. Report the observed size/digest
and request the correct user-owned input; do not edit the layout digest to bypass
this check. A different build requires separately reviewed offsets and protocol
compatibility. The synthetic profile in `tests/fixtures/` is never a fallback.
See [profile schema](docs/PROFILE.md) for validation boundaries.

For additional user-supplied evidence, `scripts/provision_inputs.py` can copy and
hash files under `game/inputs/`. It does not generate the runtime profile; the
normal fresh-install workflow above does not need that extra step.

## 2. Prepare settings and validate without running

Copy `bo2-server.example.toml` to `bo2-server.toml` only if local settings do not
already exist. Set:

- `bind`: the server's private address and the client's service port;
- `database`: persistent storage (relative to the settings file, or absolute);
- `profile`: the validated local profile path;
- `experimental_bootstrap`: explicitly `true` after explaining that this
  bootstrap binds a name-derived identity but does not prove account ownership.

Build with the pinned toolchain from `rust-toolchain.toml`; a Rust installation
and native C compiler are needed for the bundled database library. Follow the
user's existing environment policy before installing software. Then run:

```text
cargo build --locked --release
cargo run --locked --release -- --config bo2-server.toml --check-config
cargo run --locked --release -- --config bo2-server.toml --write-bundle game/startup.bundle
```

Success prints `config.valid` with league count and startup byte count. This
performs no database write and opens no listener. Missing profiles, malformed
TOML settings, malformed JSON profiles, duplicate IDs, unsafe tokens and oversized bundles are errors to fix.
The second command writes `game/startup.bundle`, an offline inspection copy;
the running server does not load it. For takeover, export to a new filename and
compare with the prior deployment before claiming byte equality. Display labels
can differ between generated defaults and a customized profile.
Do not replace a working profile merely to obtain equal bytes.

The database is created and seeded on the first authorized server start, outside
`game/` at the configured path. Do not ship a preseeded database or test wins. A
successful config check confirms syntax/rendering, not connectivity or existing
database rule compatibility.

## 3. Discover and route the required names

Read `hosts.authentication` and `hosts.lobby` from the locally generated
`game/endpoints.json`, after verifying its hash against `game/setup-manifest.json`
(or the matching pair under `game/setup-review/` for an existing deployment).
These are the production application names for the supported executable. Resolve
each role to the configured server address using the DNS skill below. Do not
infer additional names from a title, public search, another build or a broad
provider suffix. Existing installations may use verified private evidence tied
to the same input digest. Keep actual names only in ignored local files and the
user's DNS setup. No hostname list is shipped in the public source tree.

The authentication and lobby paths use a length-prefixed stream service, normally
on port3074 in the tested profile. Separate datagram discovery and peer traffic
are not served by this listener. Do not redirect platform sign-in, unrelated
content, connectivity probes or whole provider domains to it.

With authorization, follow [dns-override](.claude/skills/dns-override/SKILL.md).
Map only the established application endpoints. Query the console's actual DNS
resolver for each name and confirm it resolves to `bind`'s address. Permit the
configured TCP port for participating clients; the server has no client IP allowlist. Preserve the normal
platform authentication route. Do not add Internet port forwards.

## 4. Run, verify, stop and recover

Start only when the user requests deployment/running (or has already authorized
it). A request to review or build does not start the service. Background processes
must be hidden and have separate stdout/stderr files under `runtime/`. Record
PID, absolute executable path, binary hash, settings path and log paths privately.

```text
cargo run --locked --release -- --config bo2-server.toml
```

Check `server.listening`, then a user-initiated online entry should show
`auth.candidate_sent`, `lobby.login_bound`, `lobby.ready_sent`, verified tasks and
successful replies. `lobby.keepalive_reply` should continue during a quiet match.
Check results in SQLite: an applied win has `outcome=win_recorded`; a successful
empty reply alone is not proof of an applied win. New players start at0.

Before replacing a running build, identify the exact owned process, stop it,
make a consistent SQLite backup using its backup API, verify integrity, and keep
the old executable/logs. Restart with the same database and bind address. Do not
reset wins, wipe profiles or change console/router settings during a code review.
When asked to stop, verify process identity, stop only that instance and confirm
the listener is gone. Do not restart without authorization.

## Development and publication

[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) describes the implementation boundary,
checks, evidence levels and publication gate. Use only synthetic data in public
tests. Keep wire changes separate from operator data changes. Do not commit or
push unless asked. A build or startup check is not a console acceptance run.
