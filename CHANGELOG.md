# Changelog

## [0.1.0] - 2026-07-14

First public alpha release.

### Added

- Self-hosted Rust classification API with single and batch checks, health probes, OpenAPI, and
  `/llms.txt` discovery.
- Hybrid spam scoring from deterministic rules, local reputation data, and a bundled ONNX residual
  model.
- Reproducible evaluation and model-training tooling with licensed public spam corpora.
- Multi-architecture GHCR image for `linux/amd64` and `linux/arm64`, including SBOM and provenance
  attestations.
- `chaffnet` TypeScript SDK for browsers, Node.js 20+, and Bun.
- `chaffnet-mcp` stdio server exposing the read-only `check_content` tool.
- Optional single-instance hosted mode with tenant authentication, privacy-preserving reputation,
  and idempotent spam/ham feedback.

### Known limitations

- This is an early alpha; APIs and package contracts may change before 1.0.
- The trained model targets spam. Slop scoring does not yet have a defensible labeled corpus or
  published quality claim.
- Hosted mode does not yet include billing, anonymous signup, programmatic tenant/key issuance, or
  horizontally shared storage.

[0.1.0]: https://github.com/iuliandita/chaffnet/releases/tag/v0.1.0
