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

For the supported executable, the guide uses a reproducible local generator:

```text
python scripts/prepare_game.py --executable "<user's already decrypted multiplayer executable>"
```

Python 3.11+ is required. The helper verifies the exact input digest and generates
the profile, DNS endpoint file, input copy and manifest under ignored `game/`.
It does not require a private research checkout or guess identifiers. See the
[preparation guide](docs/PREPARATION.md) for every output and validation step.

```text
bo2-server.example.toml     # settings template
bo2-server.toml             # local settings; ignored
game/profile.json           # verified local client profile; ignored
game/inputs/                # user-provided files; ignored
game/endpoints.json         # generated DNS guidance; ignored
game/setup-manifest.json    # generated input/output hashes; ignored
runtime/                   # database, backups and logs; ignored
```

The public fixture at `tests/fixtures/profile.json` contains invented identifiers
for tests; it is not a working profile for a retail client. An existing verified
local profile is preserved during migration. Unsupported executable digests
require further compatibility research; the helper refuses them before writing.

## Build and run

Use the pinned Rust toolchain and a native C compiler. Copy the example settings
to `bo2-server.toml`, supply the profile and bind address, and explicitly enable
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
bundle locally without starting the service. Stop a foreground run with Ctrl+C.
See [deployment](docs/DEPLOYMENT.md) for backups, logs and troubleshooting.

## Verification and publication

```text
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python scripts/check_publish_tree.py
python scripts/test_publish_tree.py
python scripts/test_prepare_game.py
```

[Current status](docs/CURRENT.md) records the latest checks and limits.
[Development](docs/DEVELOPMENT.md), [profile schema](docs/PROFILE.md),
[protocol](docs/PROTOCOL.md) and [dependencies](docs/DEPENDENCIES.md) describe the
maintained public interfaces. Client-derived inputs, private research and runtime
data must remain ignored even if they are text rather than binaries.

No project distribution license has been selected. Dependency licenses and
notices remain their authors' own; generic naming does not change those terms.
