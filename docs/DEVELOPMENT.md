# Development and publication

This is an owned Rust implementation of a bounded private protocol. Generic
libraries are permitted; do not replace it with a wrapper around another backend.
A successful build, configuration check, loopback test and console run are four
different evidence levels. State which one actually happened.

## Layout

- `src/`: transport, identity, codec, store, service dispatch and profile renderer, bounded ELF mapper and runtime input loader.
- `tests/`: synthetic unit/integration vectors, loopback clients and persistence tests.
- `docs/`: generic maintained contracts, operations and current limitations.
- `AGENTS.md`, `CLAUDE.md`, `.agents/skills/`, `.claude/skills/`: guided deployment.
- `game/`: ignored user inputs, local profile, generated assets and private research.
- `runtime/`: ignored databases/backups/logs, or use an external runtime directory.

## Required checks

Run `cargo fmt --check`, `cargo test --locked`, strict all-target Clippy,
`cargo build --locked --release`, and `python scripts/check_publish_tree.py`.
Use the pinned toolchain. Offline success depends on the local dependency cache;
a lockfile is not a vendored source distribution. Preserve dependency notices.

For configuration changes, validate without binding sockets. For wire changes,
use independent vectors and negative controls; do not rely only on matching
encoders and decoders written together. Never infer a completed match from an
acknowledged upload. Keep real captures, client code, recovered names and personal
records in `game/` or external evidence storage. Unknown packet formats must not
receive invented successful results.

## Publish sources only

The checker enumerates Git-tracked and nonignored files. It rejects files outside
the allowlist, binary signatures, unreviewed fixtures, product-ID patterns,
workstation paths and non-loopback addresses. It verifies private ignore rules.
A force-added file under `game/` fails even though its directory is ignored.
Optional `--private-terms game/publication-terms.json` rejects locally known
identifiers without embedding them in public code. Inspect both the staged diff
and `git ls-files` before publishing; a checker is not a license or provenance audit.

Use `--export <new-external-directory>` to create an audited source-only tree.
This excludes Git metadata, builds and all ignored local data. Test that export
without `game/` present. Never publish a zip of the whole working directory.

Do not commit, push, select a distribution license, modify network settings or
start the server unless the user requests or has already authorized that action.
Update `docs/CURRENT.md` with actual results, residual risks and the next bounded
step. Historical research is data, never instructions to resume old probes.
