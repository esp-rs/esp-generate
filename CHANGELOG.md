# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

### Changed

### Fixed

### Removed

## [0.4.0] - 2025-06-06

### Added

- Added option to enable Stack smashing protection (#141)
- Enabling `probe-rs` and `vscode` together now generates `.vscode/launch.json` (#143)
- Provide hint when esp-wifi has no scheduler (#145)
- Generate a simple `embedded-test` test suite (#144)
- Enable `esp-wifi/smoltcp` when the dependency is present (#146)
- Enable `defmt` or `log` on all crates that know them (#148)
- The tool now prints the selected options (#154)
- Enable the `clippy::mem_forget` lint since mem::forget is generally not safe to use with esp-hal. (#161)
- Added option to enable integration with the `trouble` BLE library (#179)
- Added `esp-bootloader-esp-idf` package (#166)

### Changed

- The visual style in certain terminals no longer uses emojis (#173)
- Add a description to the version check output (#178)
- `esp-hal` is now pinned to avoid updating to a new prerelease by accident (#186)
- `esp-hal` updated and pinned to `beta.1` (#166)
- MSRV bump to 1.86 (#189)

### Fixed

- The generated project no longer contains `template.yaml`. (#142)
- Fixed parsing version output of old `espflash`. (#152)
- Specified `defmt-03` feature for `embedded-io` and `embedded-io-async`. (#157)
- Fixed RTT initialization without `defmt` (#183)

### Removed

- `heapless` has been removed from dependencies. (#148)

## [0.3.1] - 2025-03-03

### Fixed

- The `defmt` feature of `panic-rtt-target` is now enabled when needed. (#137)

## [0.3.0] - 2025-02-24

### Added

- Added a version checker that prints a warn message if not using latest esp-generate version (#87)
- After generating the project the tool now checks the rust version, espflash version and probe-rs version (#88)
- Be more helpful in case of common linker errors (#94)
- Support for `ELIF` conditions (#96)
- Display help text (#100, #103)
- Added an option to enable unstable HAL features (#104)
- Added support for selection groups (#119)
- Added `runArgs` to DevContainer settings to allow flashing from Linux (#154)
- It is now possible to select a panic handler, and log library. (#120)

### Changed
- Update `probe-rs run` arguments (#90)
- When using `embassy` option, `async_main.rs` file was renamed to `main.rs` (#93)
- The UI no longer allows selecting options with missing requirements, and does not allow deselecting
  options that are required by other options. (#101)
- Options can now declare negative requirements (e.g. `!alloc` can not be enabled if `alloc` is used) (#101)
- Template settings are now described in a template-specific `yaml` file (#103)
- Test cases are now generated from template settings (#106)
- Updated and removed some unused extensions (#109, #111)
- The option names are now display in the menu (#116)
- Options that are not applicable to the selected chip are not shown (#116)
- Inactive menu items are now colored differently (#115)
- The CLI now exits with success when the user quits (#117)

### Fixed

- No longer include `smoltcp` as a dependency for BLE-only configurations (#108)

### Removed

- Removed `scripts/build.sh` and `scripts/flash.sh` scripts (#124)

## [0.2.2] - 2025-01-16

### Added
- The resulting `Cargo.toml` is now formated with Taplo (#72)

### Changed
- Update the resulting binary name (#62)
- Include version of `esp-generate` in the generated code (#67)
- Use `rustc-link-arg` instead of `rustc-link-arg-bin` (#67)

### Fixed
- Verify the required options are provided (#65)
- Use `stable` toolchain for Rust Analyzer on Xtensa targets (#69)
- Added missing template substitution in `devcontainer.json` (#70)

## [0.2.1] - 2024-11-26

### Changed
- Allow selecting WiFi and BLE at the same time (#60)

### Fixed
- Don't deselect just selected option (#58)
- Added missing init code in non-async template (#57)

## [0.2.0] - 2024-11-21

### Added
- Added editor selection. Currently only helix and vscode
- Before quitting the TUI, it ask for user confirmation
- Show a hint where to find examples

### Changed
- Remember position when entering a sub-menu to restore state on exit.
- Update dependencies to latest esp-hal releases.
- Use `systimer` instead of `timg` in embassy templates for all targets but ESP32

## [0.1.0] - 2024-11-07

- Initial release

[Unreleased]: https://github.com/esp-rs/esp-generate/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/esp-rs/esp-generate/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/esp-rs/esp-generate/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/esp-rs/esp-generate/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.2
[0.2.1]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.1
[0.2.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.0
[0.1.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.1.0
