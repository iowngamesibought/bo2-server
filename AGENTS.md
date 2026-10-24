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

## 1. Select the local executable

Use the [pick-game-files](.claude/skills/pick-game-files/SKILL.md) skill to select
**the already decrypted multiplayer executable from the user's installed update**.
Reuse an already identified local file. The supported input is ELF64 big-endian;
its exact size and SHA-256 are in `SUPPORTED_BUILD` in `src/client_build.rs`. No archive is needed
for this supported path. A renamed encrypted file or different build is invalid.

Project-specific selection overrides the shared skill's generic main `.BIN`
and archive examples: this server needs the multiplayer component, which may
have another filename. Do not pick the boot executable just by its extension.
Within the user-provided update files, compare candidate byte length and SHA-256
with `input_bytes` and `input_sha256` in that Rust build definition. Retain the exact match;
if none matches, report the unsupported input rather than choosing a nearby file.
Do not request base-game archives or repeat already answered setup questions for
this executable-only path.

Keep the input under ignored `game/inputs/client.elf`. Copy it without changing
its bytes; do not overwrite a different existing file. A validated existing path
under `game/` can be used directly. `scripts/provision_inputs.py` is an optional
copy/hash helper; see its `--help`. No decompiler, private research folder or
prebuilt bundle is needed for a supported input.
Create `game/inputs/` before copying. On Windows, forward-slash paths or TOML
single-quoted literal paths avoid backslash escapes. Never point the server at
the user's only original if it is on temporary/removable storage; keep the
verified private copy available throughout deployment.

The Rust server checks identity, parses ELF load segments and maps reviewed
virtual-address ranges to file bytes at startup. The Rust build module contains
addresses, lengths, a digest and owned policy; it contains no extracted tokens.
This is a hybrid supported-build loader, not universal offset-free discovery.
Some settings/category keys combine reviewed fragments. Display labels are owned
generic policy. Unknown builds require separate compatibility research; never
edit the digest to bypass validation or use synthetic fixtures as a fallback.

## 2. Prepare settings and validate without running

Copy `bo2-server.example.toml` to `bo2-server.toml` only if settings do not exist.
Set `bind` to the private listener address, `database` to persistent storage,
and `executable` to the local input. Paths resolve from the settings directory.
Keep the supported service port3074. The example's `127.0.0.1` is suitable for
offline checks only when testing from a console; replace it with the server's
console-reachable private IPv4 address before deployment. Wildcard binds are
rejected. This listener does not provide DNS, so its address alone is not a
replacement console DNS resolver; use step 3 to configure the actual resolver.
Set `experimental_bootstrap = true` explicitly after explaining that it binds a
name-derived identity but does not prove account ownership.

Use exactly one input source:

- Preferred for new installations: `executable = "game/inputs/client.elf"`.
  Rust derives the profile and startup bundle in memory on every startup.
- Existing customized installations: `profile = "game/profile.json"`.
  Keep their labels, gameplay rules and database path. No executable is required
  at runtime in this compatibility mode. Never silently switch their policy.

Build with `rust-toolchain.toml` and a native C compiler for the bundled database.
Follow the user's environment policy before installing software. In executable
mode, run these commands from the project root:

```text
cargo build --locked --release
cargo run --locked --release -- --config bo2-server.toml --check-config
cargo run --locked --release -- --config bo2-server.toml --write-endpoints game/endpoints.json
cargo run --locked --release -- --config bo2-server.toml --write-bundle game/startup.bundle
```

`config.valid` reports league count and rendered startup bytes. All three offline
operations avoid database access and listeners. Export commands create new files
and refuse existing destinations; use a fresh comparison filename on takeover.
Export paths resolve from the command's working directory, unlike TOML input
paths. Create the output parent directory first, including `game/setup-review/`
when used. Repeated exports need new filenames; a partial failed output is not
a valid setup artifact. Preserve it for diagnosis and retry to a new filename.
A missing/wrong input, malformed settings or invalid extracted data is an error.
Successful rendering does not prove database compatibility or console acceptance.

| File | Producer and consumer |
| --- | --- |
| `game/inputs/client.elf` | User-provided verified input; required by runtime executable mode. |
| `game/endpoints.json` | Rust `--write-endpoints`; needed for DNS guidance, not read by the service. Includes source digest and endpoint roles. |
| `game/startup.bundle` | Optional `--write-bundle` inspection copy; runtime renders its own bytes. |
| `game/profile.json` | Optional `--write-profile` export for inspection/customization; runtime reads it only when `profile` is configured. |

Preparation and exports use the Rust executable only. There is no separate JSON
recipe or Python extraction step. `scripts/provision_inputs.py` remains an optional
copy/hash utility, not an extractor. See [preparation](docs/PREPARATION.md) and
[profile schema](docs/PROFILE.md).

A minimal fresh deployment has the built server, `bo2-server.toml` and the verified
executable. Supported-build addresses and owned defaults live in Rust source and
are compiled into the binary. `endpoints.json` supplies DNS instructions; exported
`profile.json` and `startup.bundle` are optional and can be absent on startup.
Missing exports are not a reason to change to profile mode. The database is created
at the configured path on normal startup.

For takeover, run the existing configuration check first. Use a separate config
and new output filenames under `game/setup-review/` to compare executable-mode
defaults. Preserve the existing local config and database. Compare identifiers,
IDs, rules, rotations and bundle bytes; generic display labels can differ.
The database is created/seeded only on an authorized normal start. New players
start at zero; never distribute preseeded test wins.

## 3. Discover and route the required names

Read `hosts.authentication` and `hosts.lobby` from the fresh local
`game/endpoints.json`. Confirm its `source_sha256` matches the reviewed input.
For an existing profile-only deployment, use a separate executable-mode config
to export endpoints into a new file under `game/setup-review/`.
A customized profile alone does not identify DNS names. Previously generated
files without a source digest must be checked using their matching setup manifest.

Only these established application roles are candidates for overrides. Do not
infer names from a title, string search, another build or a broad provider suffix.
Keep actual names only in ignored local output and the user's DNS setup. The
listener serves length-prefixed TCP, normally port3074. Separate datagram discovery,
peer traffic and platform sign-in are not replaced by this listener.
The authentication endpoint is contacted first, then the lobby endpoint, both
on the exported TCP port. The file contains application service names, not
provider-wide roots. Do not assume other installations on the network should
also be redirected; choose console-scoped overrides when shared use is uncertain.

With authorization, follow [dns-override](.claude/skills/dns-override/SKILL.md).
Map the exact application endpoints to the configured server address, then query
the console's actual resolver to verify each answer. Permit the configured TCP
port for participating clients; no client IP allowlist is needed. Preserve normal
platform authentication and unrelated endpoints. Do not add Internet forwards.

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
