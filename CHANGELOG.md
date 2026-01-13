# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

### Changed

### Fixed

### Removed

## [1.2.0] - 2025-01-13

### Added

- Prompt user to install a missing or outdated tool (#270)
- Show the esp-* crate versions in the help message (#276)

### Fixed

- Add .DS_Store file to .gitIgnore on mac (#285)
- Optimize toolchain scanning and filtering process (#286)
- Fix rust-toolchain.toml template (#282)

## [1.1.0] - 2025-12-11

### Added

- Enable https://rust-lang.github.io/rust-clippy/master/index.html#large_stack_frames with a threshold of 1024 (#241)
- Add an option for custom toolchain and interactive toolchain selection (#268)

### Fixed

- `"rttEnabled": true` is now set even if `defmt` is not enabled (#255)
- Fix examples link in `main.rs` (#258)

## [1.0.1] - 2025-11-05

### Changed

- Update `embedded-test` dependency to 0.7.0 (#251)
- Use `esp_hal::ram(reclaimed)` attribute for heap allocator (#252)

### Fixed

- Add App Descriptor macro to tests (#251)
- ESP32 `dram2` size (#252)

## [1.0.0] - 2025-10-30

### Added

- Generate settings for Neovim (#246)

### Changed

- Helix config: set cargo.allTargets to false (#247)
- Updated dependencies for esp-hal@1.0.0 releases (#248)

## [0.6.0] - 2025-10-14

### Added

- Add Wi-Fi/BLE setup code to the blocking template (#233)
- Check for the `esp-config` tool (#221)

### Changed

- Update bt-hci and trouble-host dependencies
- The generated project now uses Rust 2024 (#233)
- The generated project now uses all of `.dram2_uninit` for its heap (#234)
- Update templates and their dependenies to meet `rc1` of `esp-hal` (#237)
- Bump MSRV in templates to 1.88 (#237)

### Fixed

- Fix cases where padding overflow caused panic if terminal size changed (#228)

### Removed

- Remove devcontainer support (#425)

## [0.5.0] - 2025-07-16

### Added

- Added interactive flow option instead of setting target chip and name in arguments (#196)
- Added `rust-version` to the generated Cargo.toml (#192)
- Generate settings for Zed (#200)
- Updated dependencies for new esp-hal 1.0.0-rc.0 release (#215)

### Changed

- The generated project no longer enables `static_cell/nightly` (#199)
- Use `anyhow::bail` instead of log + exit (#204)

### Fixed

- Test source is no longer generated if not needed (#201)
- Conflicting options specified via `-o` are now rejected (#213)

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

[Unreleased]: https://github.com/esp-rs/esp-generate/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/esp-rs/esp-generate/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/esp-rs/esp-generate/compare/v1.0.1...v1.1.0
[1.0.1]: https://github.com/esp-rs/esp-generate/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/esp-rs/esp-generate/compare/v0.6.0...v1.0.0
[0.6.0]: https://github.com/esp-rs/esp-generate/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/esp-rs/esp-generate/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/esp-rs/esp-generate/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/esp-rs/esp-generate/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/esp-rs/esp-generate/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.2
[0.2.1]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.1
[0.2.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.0
[0.1.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.1.0
