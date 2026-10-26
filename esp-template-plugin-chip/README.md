# `esp-template-plugin-chip`

[![Crates.io](https://img.shields.io/crates/v/esp-template-plugin-chip?labelColor=1C2C2E&color=C96329&logo=Rust&style=flat-square)](https://crates.io/crates/esp-template-plugin-chip)
[![docs.rs](https://img.shields.io/docsrs/esp-template-plugin-chip?labelColor=1C2C2E&color=C96329&logo=rust&style=flat-square)](https://docs.rs/esp-template-plugin-chip)
![MSRV](https://img.shields.io/badge/MSRV-1.86-blue?labelColor=1C2C2E&logo=Rust&style=flat-square)
![Crates.io](https://img.shields.io/crates/l/esp-template-plugin-chip?labelColor=1C2C2E&style=flat-square)

Espressif chip data as an [`esp-template-sdk`] plugin for [`esp-generate`]: the chip list, the `chip` selection group, and the `chip.…` facts from [`esp-metadata-generated`].

The crate version names the set of chip facts it provides. A template pins it in its `metadata.toml`, for example `plugins = { chip = "0.5.0" }`.

[`esp-template-sdk`]: https://crates.io/crates/esp-template-sdk
[`esp-generate`]: https://github.com/esp-rs/esp-generate
[`esp-metadata-generated`]: https://crates.io/crates/esp-metadata-generated

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
