# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

### Changed
- Allow selecting WiFi and BLE at the same time

### Fixed

### Removed

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

[Unreleased]: https://github.com/esp-rs/esp-generate/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.2.0
[0.1.0]: https://github.com/esp-rs/esp-generate/releases/tag/v0.1.0
