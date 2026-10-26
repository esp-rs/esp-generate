# `esp-template-sdk`

[![Crates.io](https://img.shields.io/crates/v/esp-template-sdk?labelColor=1C2C2E&color=C96329&logo=Rust&style=flat-square)](https://crates.io/crates/esp-template-sdk)
[![docs.rs](https://img.shields.io/docsrs/esp-template-sdk?labelColor=1C2C2E&color=C96329&logo=rust&style=flat-square)](https://docs.rs/esp-template-sdk)
![MSRV](https://img.shields.io/badge/MSRV-1.86-blue?labelColor=1C2C2E&logo=Rust&style=flat-square)
![Crates.io](https://img.shields.io/crates/l/esp-template-sdk?labelColor=1C2C2E&style=flat-square)

The versioned template contract for [`esp-generate`]: option-tree parsing, the relationship engine, and the file-directive processor.

A template declares the contract version it is written against with `sdk_version` in its `metadata.toml`, and `esp-generate` refuses a template whose version this crate cannot satisfy.

[`esp-generate`]: https://github.com/esp-rs/esp-generate

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
