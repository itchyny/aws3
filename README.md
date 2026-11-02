# aws3
[![CI Status](https://github.com/itchyny/aws3/actions/workflows/ci.yaml/badge.svg?branch=main)](https://github.com/itchyny/aws3/actions/workflows/ci.yaml?query=branch:main)
[![crates.io](https://img.shields.io/crates/v/aws3.svg)](https://crates.io/crates/aws3)
[![release](https://img.shields.io/github/release/itchyny/aws3/all.svg)](https://github.com/itchyny/aws3/releases)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/itchyny/aws3/blob/main/LICENSE)

A fast command compatible with `aws s3`, written in Rust.

The goal is a drop-in replacement for `aws s3`: replace `aws s3` with `aws3`
and your scripts keep working, with faster startup and a single binary.

## Installation

### Prebuilt binaries

Download the archive for your platform from the
[releases page](https://github.com/itchyny/aws3/releases).

### Build from source

```sh
cargo install --git https://github.com/itchyny/aws3
```

## Bug Tracker

Report bugs at [Issues - itchyny/aws3 - GitHub](https://github.com/itchyny/aws3/issues).

## Author

itchyny (<https://github.com/itchyny>)

## License

This software is released under the MIT License, see [LICENSE](LICENSE).
