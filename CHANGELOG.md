# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed 

- Restrict `Send` and `Sync` trait implementations to disallow non thread-safe Request and Replies ([#11][])

[#11]: https://github.com/trussed-dev/interchange/pull/11

## [0.3.0][] - 2023-02-01

- Remove usage of macros and replace it with `const` and generics ([#5][])
  - Add tests using [loom][]

[loom]: https://github.com/tokio-rs/loom
[#5]: https://github.com/trussed-dev/interchange/pull/5

## [0.2.2][] - 2022-08-22

- Soundness fix ([#4][])

[#4]: https://github.com/trussed-dev/interchange/pull/4

## [0.2.0][] - 2021-04-23

- Changes API to use references instead of moves.
  This improves stack usage.

[Unreleased]: https://github.com/trussed-dev/interchange/compare/0.3.0...HEAD
[0.3.0]: https://github.com/trussed-dev/interchange/compare/0.2.2...0.3.0
[0.2.2]: https://github.com/trussed-dev/interchange/compare/0.2.0...0.2.2
[0.2.0]: https://github.com/trussed-dev/interchange/compare/0.1.2...0.2.0
