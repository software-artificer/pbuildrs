# pbuildrs
A Protocol Buffers builder for Rust (pb builder rs).

## What is this?
This is a small CLI tool that is designed to do two things:
1. Patch any Protobuf files that use
[editions](https://protobuf.dev/editions/overview/) instead of the `proto2` or
`proto3` syntax.
1. Generate a proper set of Rust modules that reflect the package structure of
the source Protobuf definitions.

It uses [prost](https://github.com/tokio-rs/prost) Protobuf implementation via
the [tonic-prost-build](https://crates.io/crates/tonic-prost-build) crate.

## Why is this necessary?
1. Prost does not currently support
[editions](https://github.com/tokio-rs/prost/issues/1031). Some work is being
done there, but until it is completed, we can't compile `edition`-enabled files
with Prost. For the use cases this project is currently used, it is sufficient
to simply swap `edition = "2023";` with `syntax = "proto3";`, but it will not
work as intended in all cases, for example in projects that take advantage of
the edition features.
1. Prost generated code produces "flat" source files, one per package name. It
is designed to be run in `build.rs` and subsequently included into the source
code of the project using the `include!(...)` macro. This works in some
situations, but doesn't scale well. One has to bring Protobuf files into each
project where it needs to be used, maintain the `build.rs` file, ensuring that
module structure is correct, incurr additional penalty at build times, have
`protoc` installed in their environments, yada, yada. With `pbuildrs`, one can
generate the source code with a proper module structure that can be turned into
a library/crate and reused across projects as needed.

## License
This project is licensed under the [MIT License](LICENSE.md).

## Contribution
For contribution guidelines see [CONTRIBUTING.md](CONTRIBUTING.md).
