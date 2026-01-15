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
      esp-generate
      ```
      You will be prompted to select a target chip and name for your project, after which you would use TUI to select the other options you need for your project.

   2. Using the Command Line Interface (CLI), adding the options to the `esp-generate` command:

      ```
      esp-generate --chip esp32 -o alloc -o wifi your-project
      ```
      Use the `--headless` flag to avoid using the TUI.

      Replace the chip and project name accordingly, and select the desired options using the `-o/--option` flag.
      Use the `esp-generate list-options` command to see a list of available options. Use `esp-generate explain <option>` to get a detailed explanation of an option.

[release binaries]: https://github.com/esp-rs/esp-generate/releases
[`cargo-binstall`]: https://github.com/cargo-bins/cargo-binstall

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
