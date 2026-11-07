# Changelog
## [v0.2.0](https://github.com/itchyny/aws3/compare/v0.1.0..v0.2.0) (2026-11-07)
* support `--tags` option to set tags on the objects uploaded from local paths
* support `--debug` option to write the logs of the requests and the responses
* parse `--metadata` option in the shorthand syntax with lists, maps, quotes, and parameter
  files, and reject the values which are not strings and the metadata with non-ASCII
  characters
* copy annotations of the objects on multipart copies with `--copy-props all`
* presign URLs of Multi-Region Access Points with any expirations
* reject `mb`, `sync`, and case conflict detection on directory buckets
* retry the responses of errors with the 200 status code, and report the retries and the
  reason phrases of the status codes in the error messages
* report the URLs of the requests on connection and TLS errors
* report the progress of uploads and downloads more frequently

## [v0.1.0](https://github.com/itchyny/aws3/compare/b09d382..v0.1.0) (2026-11-04)
* initial implementation of the command compatible with `aws s3`
* implement all the subcommands of `aws s3`: `ls`, `cp`, `mv`, `rm`, `sync`, `mb`, `rb`,
  `presign`, and `website`, with the same options, outputs, and exit codes
* implement uploads, downloads, and copies with multipart transfers, streaming of the
  standard input and output, `--exclude` and `--include` filters, and dry runs
* implement `sync` with `--delete`, `--size-only`, `--exact-timestamps`, and
  `--case-conflict`
* support the properties of objects: metadata, content headers, storage classes, ACLs and
  grants, tags with `--copy-props`, and server-side encryption of SSE-S3, SSE-KMS, and SSE-C
* support checksums of CRC64NVME by default and `--checksum-algorithm`, with the validation
  of full object checksums on multipart downloads, `request_checksum_calculation`, and
  `response_checksum_validation`
* load the credentials, the region, and the settings in the same way as the AWS CLI, from
  the environment variables, profiles, SSO, credential process, and `aws login`
* support the settings of the `s3` section: `multipart_threshold`, `multipart_chunksize`,
  `max_concurrent_requests`, `max_queue_size`, `max_bandwidth`, `addressing_style`,
  `use_accelerate_endpoint`, `use_dualstack_endpoint`, `use_arn_region`,
  `s3_disable_multiregion_access_points`, and `payload_signing_enabled`
* support custom endpoints and S3-compatible storage, CA bundles, and `--no-verify-ssl`
* support access point ARNs, Multi-Region Access Points, and S3 on Outposts access points,
  with `--validate-same-s3-paths` to prevent moving objects onto themselves
* redirect the requests to the regions of the buckets
* pin the ETags of multipart downloads and copies to detect the objects modified during
  the transfers
* validate bucket names, paths, and transfer settings with the same errors as the AWS CLI
* support `--cli-error-format` with the `enhanced`, `legacy`, `json`, `yaml`, `text`, and
  `table` formats
* abort multipart uploads and remove temporary files on Ctrl-C
* transfer many small files faster with 64 concurrent requests by default, and large files
  with less CPU and memory by streaming the parts
