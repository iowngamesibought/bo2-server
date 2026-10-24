# Dependencies and distribution status

Dependency versions are pinned in the manifest/lockfile. This inventory uses
the resolved package manifests for the tested native target. No project-wide
distribution license has been selected; no dependency notice was anonymized.

| Direct dependency | Version | License from its manifest |
| --- | --- | --- |
| des | 0.8.1 | MIT OR Apache-2.0 |
| hmac | 0.12.1 | MIT OR Apache-2.0 |
| rand | 0.8.5 | MIT OR Apache-2.0 |
| rusqlite | 0.40.2 | MIT |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.145 | MIT OR Apache-2.0 |
| sha1 | 0.10.6 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| tiger | 0.2.1 | MIT OR Apache-2.0 |
| toml | 0.8.23 | MIT OR Apache-2.0 |

The bundled database library compiles its upstream C implementation. Its own
license and upstream notices remain applicable. The full transitive graph is
resolved by `Cargo.lock`; use `cargo metadata` to inspect package metadata.
Offline builds were verified with this workstation's cache; no complete vendored
dependency closure is claimed. A new machine may need to fetch pinned crates.
