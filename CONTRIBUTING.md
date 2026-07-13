# Contributing to chaffnet

chaffnet is an early-alpha project. Small, focused pull requests are easiest to review.

## Before you start

- Search existing issues and pull requests before opening a new one.
- Open an issue before a large behavior or API change so the design can be agreed first.
- Report vulnerabilities through the private process in [SECURITY.md](SECURITY.md), never in a
  public issue.

## Development setup

The repository uses stable Rust, Python 3.12 with uv, and Bun 1.3.14.

```bash
bun install --frozen-lockfile
uv sync --frozen --group model
cargo build --all
```

Run the checks that CI enforces before opening a pull request:

```bash
uv run --frozen --group model python -m unittest discover -s scripts/tests -p 'test_*.py' -v
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
bun run typecheck:sdk
bun run test:sdk
bun run build:sdk
bun run pack:sdk
bun run typecheck:mcp
bun run test:mcp
bun run pack:mcp
```

## Changes and pull requests

- Branch from `main` and keep each pull request to one concern.
- Add or update tests for behavior changes and bug fixes.
- Use Conventional Commit subjects, for example `fix(server): reject empty feedback`.
- Update user-facing documentation when an API, command, configuration value, or package contract
  changes.
- Do not hand-edit generated model or reputation artifacts. Change their sources and rerun the
  documented generator.
- Include source, license, checksum, and normalization metadata for new datasets or reputation inputs.

By submitting a contribution, you agree that it is licensed under the repository's Apache-2.0
license.
