# Publication review — 2026-10-24

## Prepared tree

The public package/binary is `bo2-server`, as requested by the user. Apart from
that project name, owned source identifiers and prose are generic; client names,
product identifiers, service domains, map/mode strings,
private account details and workstation paths are not embedded in the public tree.
Toolchain/dependency names and license attribution are preserved.

Client data and prior research are under ignored `game/`. Deployment settings,
runtime data, build output and local agent settings are ignored separately.
The source publication checker rejects force-added private paths, disguised
binaries, unreviewed fixtures, product-ID patterns and local addresses/paths.
A private identifying-term scan is also applied using a list kept in `game/`.
Git history was empty and no remote was configured at inspection. Nothing has
been committed or pushed as part of this preparation.

The guide in `AGENTS.md` adapts the established agent-deployment template and
includes its shared file-selection and DNS skills in both supported discovery
locations. `CLAUDE.md` imports that same guide. The public procedure is standalone;
actual client compatibility inputs must come from the user's own installation.

## Compatibility and verification

The startup bundle generated with the private profile is byte-for-byte identical
to the prior tested bundle:9,950 bytes. Competition IDs/rules, season window,
match minimum, geographic grouping and result policy are preserved. No live
SQLite rows were changed. The server was stopped before preparation and remains
off; validation/export commands do not open listeners.

Checks completed during preparation:

- 96 Rust tests, including the new local-profile/configuration checks;
- 4 publication-gate negative tests and 4 synthetic preparation tests;
- formatting and strict all-target Clippy;
- release build with the pinned toolchain and lockfile;
- TOML configuration validation without opening the database, including exact
  equality of migrated local settings and regression cases for malformed TOML;
- byte-identical before/after startup bundle comparison;
- source allowlist, private-term scan and required ignore checks.

All checks were repeated successfully from the retained original project folder.
The final publication scan covered 76 files with zero failures.
Before the package rename and TOML migration, the source-only export was also
tested without the private `game/` folder.
This verifies that builds/tests do not silently depend on local client files.
The renamed/profile-driven build has not been re-run against consoles; preserve
that acceptance boundary until the user requests another test.

Client IP filtering was subsequently removed by request. The local and example
TOML settings omit `allow_ips`, and the accept loop only applies capacity limits.
Handshake/source-bound token checks remain. The updated suite includes a
two-client handshake without IP enrollment.

## Fresh installation preparation

The agent guide includes a reproducible generator and an exact-input layout.
Public data includes offsets/lengths and the input digest, with no extracted
client tokens. The generated profile preserves gameplay identifiers, category
keys and rules from the working profile, with generic display labels. A clean
source export is used to generate all four local files and render the startup
bundle, without a private research checkout, database write or listener.
The generated default display labels still require console acceptance. Existing
customized profiles are preserved. See [preparation](PREPARATION.md).

## Items remaining before a release

A project distribution license has not been selected. Do not choose one on the
user's behalf or remove dependency notices. Full multiplayer coverage, Internet
connectivity and independent result corroboration remain outside the tested
milestone. The local profile is required for real deployment; the synthetic
fixture is not a substitute. Publish the audited source tree, never the entire
working directory or its ignored files.
