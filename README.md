# `esp-generate`

[![Crates.io](https://img.shields.io/crates/v/esp-generate?labelColor=1C2C2E&color=C96329&logo=Rust&style=flat-square)](https://crates.io/crates/esp-generate)
![MSRV](https://img.shields.io/badge/MSRV-1.86-blue?labelColor=1C2C2E&logo=Rust&style=flat-square)
![Crates.io](https://img.shields.io/crates/l/esp-generate?labelColor=1C2C2E&style=flat-square)

Template generation tool to create `no_std` applications targeting Espressif's line of SoCs and modules.

At present, this template supports the ESP32, ESP32-C2/C3/C6, ESP32-H2, and ESP32-S2/S3. Support for additional devices will be added as they become available.

## Quickstart

To generate a project using this template:

1. Install `esp-generate`:

```
cargo install esp-generate --locked
```

You can also directly download pre-compiled [release binaries] or use [`cargo-binstall`].

2. Generate a project. There are two options:

   1. Using the Terminal User Interface (TUI):

      ```
      esp-generate --chip esp32 your-project
      ```

      Replace the chip and project name accordingly, and select the desired options using the TUI.

   2. Using the Command Line Interface (CLI), adding the options to the `esp-generate` command:

      ```
      esp-generate --chip esp32 -o alloc -o wifi your-project
      ```
      Use the `--headless` flag to avoid using the TUI.

      Replace the chip and project name accordingly, and select the desired options using the `-o/--option` flag. For a full list of available options, see [Available Options](#available-options) section of this README.

[release binaries]: https://github.com/esp-rs/esp-generate/releases
[`cargo-binstall`]: https://github.com/cargo-bins/cargo-binstall

## Available Options

- `unstable-hal`: Enables esp-hal features that may not be ready for general use yet.
- `alloc`: Enables allocations via the `esp-alloc` crate.
- `wifi`: Enables Wi-Fi via the `esp-wifi` crate; requires `alloc`.
- `ble`: Enables BLE via the `esp-wifi` crate; requires `alloc`.
- `embassy`: Adds `embassy` framework support.
- `stack-smashing-protection`: Enables [stack smashing protection](https://doc.rust-lang.org/rustc/exploit-mitigations.html#stack-smashing-protection). Requires nightly Rust.
- `probe-rs`: Replaces `espflash` with `probe-rs` and enables RTT-based options.
- `flashing-probe-rs`: Contains options that require `probe-rs`:
  - `defmt`: Adds support for `defmt` printing. Uses `rtt-target` as the RTT implementation.
  - `panic-rtt-target`: Uses `panic-rtt-target` as the panic handler.
  - `embedded-test`: Enables `embedded-test` support and generates a simple demo test case.
- `flashing-espflash`: Contains options that require `espflash`:
  - `log`: Uses the `log` library to print messages.
  - `defmt`: Adds support for `defmt` printing. Uses `esp-println` and configures `espflash` to decode `defmt` logs.
  - `esp-backtrace`: Uses `esp-backtrace` as the panic handler.
- `optional`: Enables the following set of options:
  - `wokwi`: Adds support for Wokwi simulation using [VS Code Wokwi extension].
  - `dev-container`: Adds support for [VS Code Dev Containers] and [GitHub Codespaces].
  - `ci` Adds GitHub Actions support with some basics checks.
- `editors`: Select the editor integrations:
   - `helix`: The Helix Editor
   - `vscode`: Visual Studio Code

[VS Code Wokwi extension]: https://marketplace.visualstudio.com/items?itemName=wokwi.wokwi-vscode
[VS Code Dev Containers]: https://code.visualstudio.com/docs/remote/containers#_quick-start-open-an-existing-folder-in-a-container
[GitHub Codespaces]: https://docs.github.com/en/codespaces/developing-in-codespaces/creating-a-codespace

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
