**English** · [Español](THIRD_PARTY.md)

# Third-party components

Third-party dependencies and resources included in the Bracket executable, with their origin and license. Updated in the same commit that adds, changes or removes a dependency.

Every new dependency is justified before it is added: active maintenance, a license compatible with MIT/Apache-2.0, and a measured impact on executable size and startup.

## Rust crates

Direct dependencies only; transitive ones will be reviewed with a license tool before 1.0.0.

| Crate | Version | License | Use | Measured impact |
|---|---|---|---|---|
| [ropey](https://crates.io/crates/ropey) | 1.6.1 | MIT | Text buffer (rope) in `bracket-core` | Measured once linked into `bracket.exe` (Phase 0, step 11) |

## Development and testing only

Not included in the executable.

| Crate | Version | License | Use |
|---|---|---|---|
| [proptest](https://crates.io/crates/proptest) | 1.12.0 | MIT OR Apache-2.0 | Property tests |

## Grammars and themes

No grammars or themes are included yet.

| Resource | Origin | License |
|---|---|---|
