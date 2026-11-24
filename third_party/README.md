# Maintained third-party security forks

These directories are deliberately small, source-only copies of two published crates whose
current dependency constraints retain vulnerable ZIP readers. They are Cargo `[patch.crates-io]`
overrides, so the package names and versions stay identical to the published versions and all
dependent public APIs remain unchanged.

| Directory | Upstream package | Published version | Licence | SirTone change |
| --- | --- | --- | --- | --- |
| `isideload` | `isideload` | 0.5.2 | MIT | `zip` requirement updated from `8.3` to `9.0.5` |
| `apple-codesign` | `apple-codesign` | 0.29.0 | MPL-2.0 | production and dev `zip` requirements updated from `2.2.1` to `9.0.5` |

Each directory retains its upstream README and `.cargo_vcs_info.json` provenance. The
`isideload` package declares MIT in its published Cargo metadata but does not include a licence
file in that archive, so its standard MIT text is retained here as `isideload/LICENSE`.
Do not add generated `Cargo.lock`, build directories or credentials here. Before changing either
copy: compare it to the named published source, rerun `cargo deny check`, workspace tests, and
the physical iPhone install check. This maintenance fork does **not** waive unrelated advisories,
including the upstream `chrono` maintenance advisory.

The published source archives identify upstream revisions `326e56229d0727cb6e7b34c8cf03e37e2cd3e17d`
for `isideload` and `2312b1e5bd77322c019613e0ca624e222c6c6e08` for `apple-codesign`.
