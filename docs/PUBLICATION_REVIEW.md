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
The repository now has an owner-created initial commit. This review changes only
the working tree; nothing was staged, committed or pushed by the agent. The source
export reflects current file deletions without rewriting Git history.

The guide in `AGENTS.md` adapts the established agent-deployment template and
includes its shared file-selection and DNS skills in both supported discovery
locations. `CLAUDE.md` imports that same guide. The public procedure is standalone;
actual client compatibility inputs must come from the user's own installation.

## Runtime source convention

2026-10-25: supported-build metadata and owned profile construction are ordinary
typed Rust in `src/client_build.rs`, matching the sibling project's convention.
The public extraction-config folder and duplicate Python extraction helper were
removed. No JSON recipe is embedded in the binary or needed for builds. Existing
custom profile JSON remains supported; generated profile/endpoint exports stay
private. The publication allowlist no longer includes a config directory.

Input fingerprint checks, ELF segment parsing and bounded token reads remain.
Actual recovered strings come from the user's local executable. This is the
requested hybrid architecture with known-build addresses in source, not a claim
of eliminating all fixed addresses. Shared guide/skill blocks remain unchanged.

The server is stopped. The user reported that the preceding build worked with
its custom profile; no new console acceptance is claimed for this source cleanup.
Existing deployment configuration and database are preserved.

## Items remaining before a release

A project distribution license has not been selected. Do not choose one on the
user's behalf or remove dependency notices. Full multiplayer coverage, Internet
connectivity and independent result corroboration remain outside the tested
milestone. A verified local executable or custom profile is required for deployment; the
synthetic fixture is not a substitute. Publish the audited source tree, never the entire
working directory or its ignored files.


Validation of the Rust-source cleanup:102 Rust tests,5 publication-gate tests,
formatting, strict Clippy, locked offline build and76-file source audit/export all
passed. Actual-input profile/endpoint outputs and both default/custom bundles
remain byte-identical. A clean exported source build and all offline exports
succeeded without a config directory, Python extractor, database or listener.
