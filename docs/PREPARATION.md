# Reproducible local preparation

Fresh setup needs the public source checkout, Python 3.11+, the pinned Rust
toolchain/native C compiler, and the user's already decrypted multiplayer
executable matching `config/client-layout.json`. No prior private research folder
or installed analysis engine is required. Input selection and decryption remain
the user's responsibility; see the shared file-selection skill in `AGENTS.md`.

## Preparation and acceptance sequence

1. Run `python scripts/prepare_game.py --executable "<user file>"` from the
   project root. Default output is the project's ignored `game/` directory.
   Unsupported inputs fail before files are generated. The helper prints status
   without exposing the extracted identifiers.
2. Inspect `game/setup-manifest.json`. Its relative file entries identify the
   copied executable, generated profile and DNS endpoint file, with sizes and
   SHA-256 hashes. The source digest must match the reviewed layout. The layout
   digest is calculated over its canonical JSON representation (UTF-8, two-space
   indentation, trailing newline), not arbitrary whitespace in the source file.
3. Copy the public TOML example to local `bo2-server.toml`. Set the private bind
   address and persistent database path; leave `profile = "game/profile.json"`.
   Explicitly enable the experimental bootstrap after reviewing its identity
   limitations. Existing settings must be preserved rather than overwritten.
4. Build and run `--config bo2-server.toml --check-config`. Expect `config.valid`,
   a nonzero league count and startup byte count. This neither opens the database
   nor starts the service. Then run `--config bo2-server.toml --write-bundle
   game/startup.bundle` to retain the generated startup data for inspection.
5. Read the two roles in `game/endpoints.json`; follow the DNS skill with user
   authorization. Query the console's actual resolver and verify the configured
   server address. This file is guidance, not a DNS service or automatic router
   configuration. Separate platform sign-in and datagram discovery stay untouched.
6. Start only when authorized. Startup creates/seeds the configured database and
   generates the bundle in memory. Check logs, then test online entry and a match
   with the user. Record actual acceptance separately from offline preparation.

The four generated files plus the optional inspection bundle all live in `game/`.
The only required runtime file there is `profile.json`. The TOML configuration,
database, logs and process record live outside `game/` and remain private. There
is no required downloaded asset, manually assembled publisher archive, encrypted
input or prepopulated database. New players start with zero wins.

## Existing installations

The helper refuses differing files and leaves an existing profile intact. Use
`--output game/setup-review` to generate a separate comparison. The default policy
preserves the reviewed series/playlist IDs, party rules, map/mode rotations and
minimum-player settings. Generic display labels may differ from earlier private profiles. Established
category keys are preserved and referenced consistently in the generated profile. The menu's category filter is extracted.

Do not replace a customized deployment automatically. Compare numeric rules,
map/mode lists, bundle prefix/suffix, member names, filter, category keys and setting tokens.
Retain customized labels and categories if they work. Existing published database
rules are immutable; `--check-config` does not test them against an existing DB.
For missing supporting files, use the verified endpoint/manifest pair under
`game/setup-review/`. Its manifest describes that generated comparison profile;
leave it with that profile rather than claiming its digest describes a customized one.

## Recipe provenance and limits

The reviewed layout identifies one executable by exact full-file digest and
size. Each `slices` reference is a bounded file offset/length; multi-slice entries
concatenate native string fragments for a script setting and established
operator-defined category keys from the previously working profile. The helper does not discover semantics from a string match. Required
identifier meanings come from earlier loader/serializer research and the prior
working local configuration. It copies no executable instructions into public
source. Generic display text and competition choices are owned policy, not recovered
official configuration; composed category keys preserve the existing policy.

Every prepared input is verified before extracting names. Bounds and token
syntax are checked again; the Rust configuration check validates the complete
profile and bundle. Neither check proves another build compatible, verifies
account ownership, or substitutes for a console acceptance run. The newly
generated default display labels still require that acceptance step.

To support another executable, perform targeted research and add separately
reviewed mappings and tests. Never change only the digest, scan for similar
strings and assume compatibility, or deploy the synthetic test fixture.
