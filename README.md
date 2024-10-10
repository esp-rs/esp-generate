# `esp-generate`

Template generation tool to create `no_std` applications targeting Espressif's line of SoCs and modules. At present, this template supports the ESP32, ESP32-C2, ESP32-C3, ESP32-C6, ESP32-H2, ESP32-S2, and ESP32-S3. Additional devices will be added as they become available.

> [!WARNING]
>
> This project is still in the early stages of development. If you encounter any issue or you would like to see any feature added. Please, open an [issue].

[issue]: https://github.com/esp-rs/esp-generate/issues/new

## Quickstart

To generate a proect using this template:
1. Install `esp-generate`:
```
cargo install esp-generate --git https://github.com/esp-rs/esp-template
```
2. Generate a project. There are two options:
     1. Using TUI:
        ```
        esp-generate --chip esp32 tests
        ```
        Replace the chip and project name accordingly and choose the different options using the TUI.

     2. Adding the options to the `esp-generate command:
        ```
        esp-generate --chip esp32 -o wifi -o alloc tests
        ```
        Replace the chip and project name accordingly and choose the different options using the `-o/--option` flag.
        For a full list of available options, see [Usage](#usage) section.

## Usage

```
Usage: esp-generate [OPTIONS] --chip <CHIP> <NAME>

Arguments:
  <NAME>

Options:
  -c, --chip <CHIP>      [possible values: esp32, esp32s2, esp32s3, esp32c2, esp32c3, esp32c6, esp32h2]
      --headless
  -o, --option <OPTION>
  -h, --help             Print help
  -V, --version          Print version
```

### Available Options

- `alloc`: Enables allocations via the `esp-alloc` crate.
- `wifi`: Enables Wi-Fi via the `esp-wifi` crate. Requires `alloc`.
- `ble`: Enables BLE via the `esp-wifi` crate. Requires `alloc`.
- `embassy`: Adds `embassy` framework support.
- `probe-rs`: Enables `defmt` and flashes using `probe-rs` instead of `espflash`.
- `stack-protector`: Enable stack-smash protection (`nightly` only).
- `optional`: Enables the following set of options:
  - `wokwi`: Adds support for Wokwi simulation using [VS Code Wokwi extension].
  - `dev-container`: Adds support for [VS Code Dev Containers] and [GitHub Codespaces].
  - `ci` Adds GitHub Actions support with some basics checks.

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
