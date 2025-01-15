# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- The resulting `Cargo.toml` is now formated with Taplo (#72)

### Changed
- Update the resulting binary name (#62)
- Include version of `esp-generate` in the generated code (#67)
- Use rustc-link-arg instead of rustc-link-arg-bin (#67)

### Fixed
- Verify the required options are provided (#65)
- Use stable toolchain for rust-analyzer on xtensa targets (#69)
- Added missing template substitution in devcontainer.json (#70)

### Removed

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

[Unreleased]: https://github.com/esp-rs/esp-generate/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.1
[0.2.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.0
[0.1.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.1.0
