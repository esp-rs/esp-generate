# `esp-generate`

Template generation tool to create `no_std` applications targeting Espressif's line of SoCs and modules.

At present, this template supports the ESP32, ESP32-C2/C3/C6, ESP32-H2, and ESP32-S2/S3. Support for additional devices will be added as they become available.

> [!NOTE]
>
> This project is still in the early stages of development. If you encounter any problems, or you would like to see any feature added, please [open an issue] or [start a discussion].

[open an issue]: https://github.com/esp-rs/esp-generate/issues/new
[start a discussion]: https://github.com/esp-rs/esp-generate/discussions/new/choose

## Quickstart

To generate a proect using this template:

1. Install `esp-generate`:

```
cargo install esp-generate
```

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

      Replace the chip and project name accordingly, and select the desired options using the `-o/--option` flag. For a full list of available options, see [Available Options](#available-options) section of this README.

## Available Options

- `alloc`: Enables allocations via the `esp-alloc` crate.
- `wifi`: Enables Wi-Fi via the `esp-wifi` crate; requires `alloc`.
- `ble`: Enables BLE via the `esp-wifi` crate; requires `alloc`.
- `embassy`: Adds `embassy` framework support.
- `probe-rs`: Enables `defmt` and flashes using `probe-rs` instead of `espflash`.
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
