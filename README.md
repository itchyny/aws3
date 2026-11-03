# aws3
[![CI Status](https://github.com/itchyny/aws3/actions/workflows/ci.yaml/badge.svg?branch=main)](https://github.com/itchyny/aws3/actions/workflows/ci.yaml?query=branch:main)
[![crates.io](https://img.shields.io/crates/v/aws3.svg)](https://crates.io/crates/aws3)
[![release](https://img.shields.io/github/release/itchyny/aws3/all.svg)](https://github.com/itchyny/aws3/releases)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/itchyny/aws3/blob/main/LICENSE)

A fast command compatible with `aws s3`, written in Rust.

The goal is a drop-in replacement for `aws s3`: replace `aws s3` with `aws3`
and your scripts keep working, with faster startup and a single binary.

## Usage

All the subcommands of `aws s3` are supported: `ls`, `cp`, `mv`, `rm`,
`sync`, `mb`, `rb`, `presign` and `website`, with the same options, outputs
and exit codes.

```sh
aws3 ls s3://bucket/prefix/
aws3 cp ./dir s3://bucket/prefix/ --recursive --exclude '*.tmp'
aws3 sync s3://bucket/prefix/ ./dir --delete
```

The credentials, the region and the transfer settings in the `s3` section are
loaded from the same environment variables and configuration files as the AWS
CLI.

### Differences from the AWS CLI

- The error messages are prefixed with `aws3:` instead of `aws:`.
- The root certificates of Mozilla are used for TLS, unless a CA bundle is
  specified with `--ca-bundle` or `AWS_CA_BUNDLE`.
- `--debug`, `--query`, `--output` and `--color` are accepted but ignored.
- The default of `max_concurrent_requests` in the `s3` section is 64 instead
  of 10, which makes the transfers of many small files faster.
- `payload_signing_enabled = true` in the `s3` section does not sign the
  payloads of uploads over HTTPS.

## Installation

### Prebuilt binaries

Download the archive for your platform from the
[releases page](https://github.com/itchyny/aws3/releases).

### Using cargo-binstall

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), the
prebuilt binary for your platform is installed from the releases page.

```sh
cargo binstall aws3
```

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
