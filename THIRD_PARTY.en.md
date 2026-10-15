**English** · [Español](THIRD_PARTY.md)

# Third-party components

Third-party dependencies and resources included in the Bracket executable, with their origin and license. Updated in the same commit that adds, changes or removes a dependency.

Every new dependency is justified before it is added: active maintenance, a license compatible with MIT/Apache-2.0, and a measured impact on executable size and startup.

## Rust crates

Direct dependencies only; transitive ones will be reviewed with a license tool before 1.0.0.

| Crate | Version | License | Use | Measured impact |
|---|---|---|---|---|
| [ropey](https://crates.io/crates/ropey) | 1.6.1 | MIT | Text buffer (rope) in `bracket-core` | +0.3 MB together with the core and the editor view ([measurements](docs/en/measurements.md)) |
| [eframe](https://crates.io/crates/eframe) (includes egui) | 0.36.2 | MIT OR Apache-2.0 | Graphical interface (glow backend, no bundled fonts) in `bracket-ui` | Exe from 0.10 to 5.23 MB; startup from 9 to 139 ms ([measurements](docs/en/measurements.md)); without `default_fonts` the exe is 1.36 MB smaller |
| [toml_edit](https://crates.io/crates/toml_edit) | 0.25.17 | MIT OR Apache-2.0 | Reading and saving `config.toml` in `bracket-session`, keeping comments | +165 KB together with the configuration code; no measurable startup effect |
| [winreg](https://crates.io/crates/winreg) | 0.56.0 | MIT | Reading installed fonts from the Windows registry in `bracket-app` (and, in Phase 5, file associations) | Included in the step 4 change (−1.36 MB overall after removing the bundled fonts) |

## Development and testing only

Not included in the executable.

| Crate | Version | License | Use |
|---|---|---|---|
| [proptest](https://crates.io/crates/proptest) | 1.12.0 | MIT OR Apache-2.0 | Property tests |
| [criterion](https://crates.io/crates/criterion) | 0.8.2 | Apache-2.0 OR MIT | Benchmarks |

## Grammars and themes

No grammars or themes are included yet.

| Resource | Origin | License |
|---|---|---|
