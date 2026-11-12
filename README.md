# aws3

[![CI Status](https://github.com/itchyny/aws3/actions/workflows/ci.yaml/badge.svg?branch=main)](https://github.com/itchyny/aws3/actions/workflows/ci.yaml?query=branch:main)
[![crates.io](https://img.shields.io/crates/v/aws3.svg)](https://crates.io/crates/aws3)
[![release](https://img.shields.io/github/release/itchyny/aws3/all.svg)](https://github.com/itchyny/aws3/releases)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/itchyny/aws3/blob/main/LICENSE)

A fast command compatible with `aws s3`, written in Rust.

The goal is a drop-in replacement for `aws s3`: replace `aws s3` with `aws3`
and your scripts keep working, with faster startup and a single binary. The
commonly used operations of `aws s3api` are also supported by `aws3 api`.

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
CLI. The requests are sent through the proxies of `HTTP_PROXY` and
`HTTPS_PROXY` except for the hosts of `NO_PROXY`, also in lowercase, with the
credentials in the URLs of the proxies. The certificates of the proxies of
HTTPS are verified with the same CA bundle as the endpoints.

### The api subcommand

The commonly used operations of `aws s3api` are supported by `aws3 api`, with
the same options, outputs in JSON and text, and exit codes. Replace `aws s3api`
with `aws3 api` for these operations, and use `aws s3api` for the others.

| Target | Operations |
| --- | --- |
| Objects | `head-object`, `get-object`, `put-object`, `copy-object`, `delete-object`, `delete-objects` |
| Object listing | `list-objects-v2`, `list-object-versions` |
| Buckets | `list-buckets`, `head-bucket`, `get-bucket-location`, `create-bucket`, `delete-bucket` |
| Versioning | `get-bucket-versioning`, `put-bucket-versioning` |
| Public access block | `get-public-access-block`, `put-public-access-block`, `delete-public-access-block` |
| Policy | `get-bucket-policy`, `put-bucket-policy`, `delete-bucket-policy` |
| Encryption | `get-bucket-encryption`, `put-bucket-encryption`, `delete-bucket-encryption` |
| Lifecycle | `get-bucket-lifecycle-configuration`, `put-bucket-lifecycle-configuration`, `delete-bucket-lifecycle` |
| CORS | `get-bucket-cors`, `put-bucket-cors`, `delete-bucket-cors` |

```sh
aws3 api head-object --bucket bucket --key path/to/file.txt
aws3 api list-objects-v2 --bucket bucket --prefix logs/ --query 'Contents[?Size > `0`].Key'
aws3 api put-object --bucket bucket --key file.txt --body file.txt --metadata key=value
aws3 api put-bucket-policy --bucket bucket --policy file://policy.json
```

The structured arguments are accepted in the shorthand syntax and in JSON, also
from the files of `file://`, and validated with the same error messages as the
AWS CLI. The responses of the list operations are paginated automatically with
`--page-size`, `--max-items`, `--starting-token` and `--no-paginate`, and
filtered by the JMESPath expressions of `--query`.

### Differences from the AWS CLI

- Only the commonly used operations of `aws s3api` are supported by
  `aws3 api`, whose outputs are in JSON and text, not in the table and YAML of
  `--output`, and are not shown through the pager of `cli_pager` or
  `AWS_PAGER`.
- The error messages are prefixed with `aws3:` instead of `aws:`.
- The root certificates of Mozilla are used for TLS, unless a CA bundle is
  specified with `--ca-bundle` or `AWS_CA_BUNDLE`.
- The default of `max_concurrent_requests` in the `s3` section is 64 instead
  of 10, which makes the transfers of many small files faster. The parts of a
  multipart transfer are up to 10 by default, which use the bandwidth with
  less CPU.

## Installation

### Homebrew

```sh
brew install itchyny/tap/aws3
```

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
network, on Apple M4 (10 cores, 24 GiB) with macOS 27.2. The tools are AWS CLI
2.39.6, s5cmd 2.3.0, s7cmd 1.8.6 and aws3 0.3.0, with the default settings of
each tool, and with the same concurrency of 10 and 64. The results are the
means of five runs, or three runs for the files of 256 MiB, where the tools run
in turns so that the changes of the network affect every tool evenly.

By default, `aws s3` transfers with up to 10 connections and parts of 8 MiB,
aws3 transfers up to 64 files at once and up to 10 parts of 8 MiB of each file,
s5cmd transfers up to 256 files at once and up to 5 parts of 50 MiB of each
file, and s7cmd transfers up to 16 files at once and up to 16 parts of 8 MiB of
each file. The same concurrency is set by `max_concurrent_requests` in the `s3`
section for `aws s3` and aws3, `--numworkers` and `--concurrency` for s5cmd, and
`--worker-size` and `--max-parallel-uploads` for s7cmd, with the part size of
8 MiB.

The listing of objects does not depend on the concurrency. aws3 lists 10000
objects about 1.5 times faster than `aws s3`, as fast as the other tools, with
the least CPU time and memory of all, which are a fifth of `aws s3`.

With the default settings, aws3 transfers many small files four to five times
faster than `aws s3`, as fast as s5cmd for downloads and a little slower for
uploads, with about half the memory of `aws s3` and s5cmd. s7cmd is slower with
its concurrency of 16, and uses about 200 MiB of memory. With the same
concurrency of 10, the transfers of many small files take about the same time
with every tool, where aws3 uses the least CPU time and memory, with a third of
the CPU time of `aws s3` for uploads. With the concurrency of 64, aws3 and s5cmd
transfer them four to five times faster than `aws s3`, which does not get
faster.

The transfers of large files are limited by the bandwidth of the network, where
aws3 uses the least CPU time with the default settings, and a fifth to a third
of the memory of `aws s3`. aws3, s5cmd and s7cmd transfer large files slower
with 64 concurrent parts than with 10 and use more memory, so aws3 transfers up
to 64 files but up to 10 parts by default. s7cmd uses about 500 MiB of memory
for a file of 256 MiB with the concurrency of 64.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/wiki/itchyny/aws3/benchmark-dark.svg">
  <img alt="Benchmark of aws s3, aws3, s5cmd and s7cmd" src="https://raw.githubusercontent.com/wiki/itchyny/aws3/benchmark-light.svg">
</picture>

## Binary size

aws3 is a single binary of about 10 MiB on Linux, while the installer of the
AWS CLI bundles the Python runtime and the models of all the AWS services in a
directory of about 250 MiB. The archive of aws3 is about 15 times smaller to
download than the AWS CLI, and the installed size is 24 to 26 times smaller.
aws3 is also the smallest of the single binaries, about 60% of s5cmd and 40% of
s7cmd. The sizes are of the releases for Linux of AWS CLI 2.39.6, s5cmd 2.3.0,
s7cmd 1.8.6 (glibc) and aws3 0.3.0.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/wiki/itchyny/aws3/size-dark.svg">
  <img alt="Binary size of AWS CLI, aws3, s5cmd and s7cmd" src="https://raw.githubusercontent.com/wiki/itchyny/aws3/size-light.svg">
</picture>

## Bug Tracker

Report bugs at [Issues - itchyny/aws3 - GitHub](https://github.com/itchyny/aws3/issues).

## Author

itchyny (<https://github.com/itchyny>)

## License

This software is released under the MIT License, see [LICENSE](LICENSE).
