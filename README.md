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
aws3 ls s3://bucket/prefix/ --recursive --summarize
aws3 cp ./dir s3://bucket/prefix/ --recursive --exclude '*.tmp'
aws3 sync s3://bucket/prefix/ ./dir --delete --dryrun
aws3 rm s3://bucket/prefix/ --recursive
```

The credentials, the region and the transfer settings in the `s3` section are
loaded from the same environment variables and configuration files as the AWS
CLI.

### Differences from the AWS CLI

- Only the commands of `aws s3` are supported, not `aws s3api`.
- The error messages are prefixed with `aws3:` instead of `aws:`.
- The root certificates of Mozilla are used for TLS, unless a CA bundle is
  specified with `--ca-bundle` or `AWS_CA_BUNDLE`.
- The default of `max_concurrent_requests` in the `s3` section is 64 instead
  of 10, which makes the transfers of many small files faster. The parts of a
  multipart transfer are up to 10 by default, which use the bandwidth with
  less CPU.

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
cargo install aws3
```

To install the latest development version:

```sh
cargo install --git https://github.com/itchyny/aws3
```

## Benchmark

The commands were run against S3 in `ap-northeast-1` over HTTPS from a home
network, on Apple M4 (10 cores, 24 GiB) with macOS 27.2. The results are the
means of five runs, or two or three runs for the files of 1 GiB, of AWS CLI
2.39.3, s5cmd 2.3.0, s6cmd 0.0.6 and aws3, with the default settings.

aws3 uploads and downloads many small files four to five times faster than
`aws s3` with about half the memory. The transfers of large files are limited
by the bandwidth of the network, where aws3 uses about a fifth of the memory
of `aws s3`.

<details>
<summary>Results</summary>

| Operation | Tool | Wall time | CPU time | Memory |
|---|---|--:|--:|--:|
| `ls` 1000 objects | aws s3 | 0.52 s | 0.30 s | 82.9 MiB |
| | aws3 | 0.27 s | 0.05 s | 14.6 MiB |
| | s5cmd | 0.27 s | 0.08 s | 29.8 MiB |
| | s6cmd | 0.24 s | 0.05 s | 26.4 MiB |
| Upload 1000 files of 16 KiB | aws s3 | 6.97 s | 3.05 s | 92.0 MiB |
| | aws3 | 1.35 s | 1.01 s | 47.9 MiB |
| | s5cmd | 1.99 s | 0.94 s | 88.4 MiB |
| | s6cmd | 2.57 s | 1.17 s | 132.6 MiB |
| Download 1000 files of 16 KiB | aws s3 | 5.41 s | 2.89 s | 95.4 MiB |
| | aws3 | 1.22 s | 1.39 s | 47.0 MiB |
| | s5cmd | 2.79 s | 1.02 s | 93.3 MiB |
| | s6cmd | 2.55 s | 1.13 s | 119.7 MiB |
| Upload a file of 1 GiB | aws s3 | 17.80 s | 5.75 s | 242.6 MiB |
| | aws3 | 17.68 s | 3.87 s | 46.5 MiB |
| | s5cmd | 17.69 s | 4.50 s | 32.4 MiB |
| | s6cmd | 17.49 s | 3.15 s | 31.9 MiB |
| Download a file of 1 GiB | aws s3 | 9.57 s | 4.86 s | 248.9 MiB |
| | aws3 | 9.78 s | 2.86 s | 58.2 MiB |
| | s5cmd | 9.55 s | 2.90 s | 29.4 MiB |
| | s6cmd | 9.01 s | 4.72 s | 29.6 MiB |

</details>

## Bug Tracker

Report bugs at [Issues - itchyny/aws3 - GitHub](https://github.com/itchyny/aws3/issues).

## Author

itchyny (<https://github.com/itchyny>)

## License

This software is released under the MIT License, see [LICENSE](LICENSE).
