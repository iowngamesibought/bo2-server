# Local inputs and runtime extraction

New installations use the public Rust source, pinned toolchain/native C compiler
and the user's already decrypted multiplayer executable. No research checkout,
decompiler, Python installation or pre-generated profile is required for runtime
mode. File selection and decryption responsibilities are in [AGENTS.md](../AGENTS.md).

## Preferred setup

1. Create ignored `game/inputs/`, then put the supported input at `game/inputs/client.elf`, or reuse its existing
   validated local path. Preserve the original and do not replace conflicting files.
2. Copy the TOML example if settings do not already exist. Set `executable` to that
   input, select the private bind address and persistent database path, and explicitly
   enable the experimental bootstrap after reviewing its identity limitation.
   Keep port3074. Loopback suffices for offline checks; console testing needs a
   reachable private IPv4 listener address and separate DNS resolver configuration.
3. Build, then run `--config bo2-server.toml --check-config`. Paths in TOML resolve
   from the config file, independently of the process working directory.
4. Run `--config bo2-server.toml --write-endpoints game/endpoints.json`. The output
   includes the verified source digest, application roles and transport/port. Read
   the exact hostnames locally and follow the DNS skill with authorization.
5. Optionally run `--write-bundle game/startup.bundle` or
   `--write-profile game/profile.json`, with the same `--config` argument.
   These exports use new destination files; existing files are never replaced.
   Output paths are relative to the command's working directory. Create their
   private parent directory first. After a failed write, inspect/preserve any
   incomplete output and retry to a new filename.
6. Start normally only when authorized. The service derives its profile and bundle
   in memory, then opens/seeds the configured database and binds the listener.

Offline checks/exports never open a database or listener. Endpoints, profile and
bundle exports are inspection/setup aids, not required runtime files in executable
mode. They all belong under ignored `game/`. Database/logs remain in `runtime/`
or a configured external directory. No preseeded database is distributed.

Fresh startup needs only the built binary, local TOML and verified executable.
Build metadata is compiled from Rust source and requires no private copy. A generated profile,
manifest or startup bundle is not a hidden prerequisite for executable mode.
The file-selection skill's generic main-executable/archive examples do not select
this component: identify the multiplayer input by `SUPPORTED_BUILD`'s exact size/hash in `src/client_build.rs`.

## Supported-build loader

`src/elf.rs` parses ELF64 big-endian headers and file-backed load segments with
bounds/overflow/overlap checks. `src/client_data.rs` verifies the full input size
and SHA-256, maps reviewed virtual-address ranges through those segments, validates
identifiers, endpoint roles and the resulting profile, and builds startup data.
`src/client_build.rs` holds the supported identity, reviewed address ranges and
owned policy as typed Rust code, following the sibling implementation convention.
There is no separate JSON recipe, `config/` folder or generic recipe interpreter.
Client strings are still read from the user's executable, never embedded.

This hybrid design intentionally retains known-build addresses and an exact
identity gate. It is not automatic support for different builds or arbitrary
relocations. Format-relative header offsets are part of the ELF parser. Mere
string matches are not evidence of semantic roles. Composite setting/category
keys use reviewed native fragments plus owned policy; display labels are generic.
The supported build and protocol compatibility still need independent evidence.
Never weaken the identity check to accept an unknown input.

## Existing profiles

Exactly one of `executable` or `profile` must be configured. Existing customized
`profile` deployments continue unchanged and need no executable at runtime. DNS
export requires executable mode: use a separate comparison configuration instead
of guessing names from the profile. Run the Rust `--write-profile` option when a
private profile export is needed; there is no second extraction implementation.

Compare defaults separately from the current installation. Preserve configured
IDs, rules, category keys, map/mode rotations and the database path. Generic display
labels may differ from a customized profile. Do not switch a live installation
merely to obtain byte equality with defaults. Tests use invented ELF data;
real-file comparisons are kept private. Neither proves fresh console acceptance.
