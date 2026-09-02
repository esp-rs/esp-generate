# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

### Added

- Add an optional coding-agent guidance template submenu for selecting `AGENTS.md`, `CLAUDE.md`, `CODEX.md`, or `GEMINI.md`. (#335)
- Generated Xtensa projects now check for the required linker and point Unix users to source espup's export file when it is missing. (#343)
- `--template` also accepts a **repository**: `owner/repo[@branch-or-tag]`, an `https://` URL, or `git@host:path`. It is cloned shallowly with `git` (reusing your existing credentials, so private template repos work) into a temporary directory that is removed when esp-generate exits, and the resolved commit is logged so a generated project can be traced back to exactly what produced it. The template need not be the repository root — esp-generate finds the directory holding a `metadata.toml` that parses as a manifest, so a template shipped inside a larger repo works. More than one is an error listing them, never a guess. A commit SHA is not supported: shallow clones cannot take one portably. (#368)
- **Generate from an external template.** `--template <dir>` reads a template from a directory instead of the bundled one; it works with `list-options` and `explain` too. The directory is read with the same root-relative key space `build.rs` bakes in, `.git/` and `target/` are skipped, symlinks are not followed, and a path that escapes the template root is refused. Using one prints a warning, since a template decides what code and dependencies end up in your project. (#368)
- Templates now carry a `metadata.toml` manifest. It declares the `esp-template-sdk` version the template is written against — checked before any template file is read, so an incompatible template fails with one clear line — and per-file `emit` rules: `when` is the condition a file is emitted under, `as` the path it is written to. A file with no rule is emitted as-is. (#357)

### Changed

- The template contract (option tree, relationship engine, directive processor) now lives in a separate `esp-template-sdk` crate. Generated projects are unchanged. (#352)
- **Template syntax.** File directives are now rendered by [`somni-template`](https://docs.rs/somni-template). Directive lines are marked with the file's comment prefix plus `%` (`#%if`, `//%endif`, `--%else`) so ordinary comments are never parsed as directives; `#ELIF` is now `#%else if`; and `#REPLACE` is replaced by `{{ expr }}` interpolation. Generated projects are unchanged. (#352)
- `sets` keys that a template interpolates must be valid identifiers: `wokwi-board` is now `wokwi_board`, and `coding-agent-guidance-file` is now `coding_agent_guidance_file`. (#352)
- Template directives evaluate against a single fact API. `option()` and `group_selected()` are the predicates, and everything the chip decides is a field on a `chip` struct: `chip.name`, `chip.rust_target`, `chip.dram2_uninit_size`, and one field per `esp-metadata` symbol (`chip.soc_has_wifi`). A misspelled field is an error naming the file and line, not a silent `false`. Value names are `snake_case`. (#352)
- `option()` no longer matches selection-group names — use `group_selected()`. (#352)
- **Chip data is a versioned plugin.** It lives in a new `esp-template-plugin-chip` crate whose version mirrors the `esp-metadata-generated` it wraps, so the version names a symbol set rather than a release. A template declares what it needs — `plugins = { chip = "0.4.0" }` in `metadata.toml` — and pulls the chip selection group in with `!Include plugin:chip`. A binary that cannot provide a declared plugin refuses the template with one line instead of failing on unknown names part-way through rendering, and can offer several versions at once so older templates keep working. (#357)
- The option tree's chip list is generated from the chip plugin rather than carried as template YAML, so newly supported silicon appears in the menu without a template edit. (#357)
- A template's `.template/` directory is reserved for machinery — option-tree fragments and `include` partials — and is never emitted. (#357)
- What an option needs is declared in the option tree rather than hardcoded in esp-generate: `requires_nightly: true` for a nightly toolchain, `requires_tools: [probe-rs]` for a host tool. Both drive the pre-flight check before generation. (#357)
- Chip selector has been moved to the TUI. (#328)
- The chip needs to be specified as `-o <chip>` instead of `--chip <chip>` when using CLI (#328)
- Generated projects now keep runtime configuration in `.cargo/esp-config.toml` and include it from `.cargo/config.toml` (#326)
- Wi-Fi generation now requires the async Embassy template. (#341)

### Fixed

- Malformed template directives now report a `file:line` error instead of panicking or being silently ignored.
- A `requires_capabilities` entry that names no plugin namespace — `soc_has_wifi` rather than `chip.soc_has_wifi` — is now refused when the template loads. It previously read as false for every selection, silently hiding the option. (#357)
- Fixed neovim config (#336)

### Removed

- Removed the BLE `bleps` option. (#341)
- The `#%includefile` and `#%include_as` directives. Which files a generated project gets, and what they are called, is the manifest's job now — a file's own body no longer decides whether that file exists. (#357)

## [1.3.0] - 2026-04-24

### Changed

- Update `esp-hal` dependency to ~1.1.0 (#334)
- Reverted: Preserve per-method flashing options when switching probe-rs and espflash (#327)
- Flashing options are now cleared when selecting a different flashing method (#327)

## [1.3.0-rc.0] - 2026-04-17

### Added

- VSCode debugger launch configuration now sets `verifyBeforeFlashing` to `true` (#290)
- esp-generate adds the selected template parameters to the generated code (#293)
- Add `explain` and `list-options` subcommands (#293)
- Add module selector (#289)
- Add C5 and C61 support (#314)

### Changed

- esp-generate now recommends probe-rs version 0.31.0 or newer (#296)
- The default panic handler has been updated to print the panic info using the configured log method (#307)
- The generated project no longer includes default optimization settings (#310)
- Downgrade `ratatui` (#316)
- Updated for esp-hal@1.1 and esp-radio@1.0-beta.0 (#314)

### Fixed

- Fix IDE settings and checks when using `-a/--name` (#302)
- Fix UI-breaking warnings (#315)
- Preserve per-method flashing options when switching probe-rs and espflash (#312)
- Fix crash when trying to enter an empty category (#321)

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

[Unreleased]: https://github.com/esp-rs/esp-generate/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/esp-rs/esp-generate/compare/v1.2.0...v1.3.0
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
