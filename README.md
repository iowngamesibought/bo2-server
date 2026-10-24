# bo2-server

An experimental private multiplayer and competition backend. It provides a
separate player pool while leaving platform sign-in with the platform service.
This project is independent of the client's developer and publisher.

The public tree contains owned Rust code, generic deployment instructions and
synthetic tests. Client-specific names and assets are supplied locally under
ignored `game/`; player data is stored in a persistent SQLite database.

## Current scope

A tested local profile supports online entry, two-console matchmaking, a completed
match, persisted wins and a user-confirmed five-win achievement after a seeded
four-to-five test. New players start at0. Automated tests cover natural zero-to-five
progression, duplicate uploads and database reopen. This is not certification of
all clients, platforms, Internet connectivity or every multiplayer feature.

Current policy is one shared division and geographic group, two-player match
start, wins-only League statistics, and a long-running season ending before the
client's signed32-bit time limit in2038. All other League statistics retain
placeholder defaults. The bootstrap is experimental and is not proof of account
ownership; use a small private network, not an Internet-facing service.

## Agent-assisted setup

Ask an AI agent to follow [AGENTS.md](AGENTS.md). It uses the user's files to
identify the required client data and endpoints, validates local settings,
explains the DNS override, and verifies deployment. No client data, service
hostnames or decryption instructions are distributed with this repository.

New installations load the user's executable directly at startup. The Rust loader
verifies its identity, parses ELF load segments and reads reviewed virtual-address
ranges. This hybrid design retains supported-build addresses while keeping all
client-derived content local. See [preparation](docs/PREPARATION.md).

```text
bo2-server.example.toml     # public settings template
bo2-server.toml             # local settings; ignored
game/inputs/client.elf      # runtime input in executable mode; ignored
game/profile.json           # optional custom profile/inspection export; ignored
game/endpoints.json         # exported DNS guidance; ignored
runtime/                   # database, backups and logs; ignored
```

Existing custom profiles remain supported. Use exactly one of `executable` or
`profile` in TOML. Build metadata and owned defaults live in Rust source; no
separate extraction-config folder is shipped. Synthetic fixtures are never a fallback
for missing client files. Unknown executable digests fail before serving.

## Build and run

Use the pinned Rust toolchain and a native C compiler. Copy the example settings
to `bo2-server.toml`, supply the executable path and bind address, and explicitly enable
the experimental bootstrap. Paths resolve relative to the settings file.
Any client that can reach the listener may connect; no client IP allowlist is required.
Server settings use TOML; the separate client profile and test fixtures use JSON.

```text
cargo build --locked --release
cargo run --locked --release -- --config bo2-server.toml --check-config
cargo run --locked --release -- --config bo2-server.toml
```

`--check-config` renders/validates startup data without opening a listener or
writing the database. `--write-bundle game/startup.bundle` exports the same
bundle locally without starting the service. Executable mode also provides
`--write-endpoints game/endpoints.json` and `--write-profile game/profile.json`.
Exports require new filenames and never replace existing files. Stop a foreground
run with Ctrl+C.
See [deployment](docs/DEPLOYMENT.md) for backups, logs and troubleshooting.

## Verification and publication

```text
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python scripts/check_publish_tree.py
python scripts/test_publish_tree.py
```

[Current status](docs/CURRENT.md) records the latest checks and limits.
[Development](docs/DEVELOPMENT.md), [profile schema](docs/PROFILE.md),
[protocol](docs/PROTOCOL.md) and [dependencies](docs/DEPENDENCIES.md) describe the
maintained public interfaces. Client-derived inputs, private research and runtime
data must remain ignored even if they are text rather than binaries.

No project distribution license has been selected. Dependency licenses and
notices remain their authors' own; generic naming does not change those terms.
