# chaffnet

Spam and AI-slop classification for user-generated content. Open-source engine, optional hosted API.

`chaffnet` scores a piece of content (a comment, review, or forum post) and returns two
values normalized to 0..1 plus machine-readable reason codes:

- **spam** — likelihood the content is spam/abuse.
- **slop** — likelihood the content is AI-generated low-value filler.

It is built for non-WordPress stacks and for AI agents: a single stateless endpoint, structured
output you can branch on, and a hosted option when you do not want to run the infrastructure.

## Why

Akismet is the default for comment spam, but it is WordPress-centric, a closed black box, and
awkward outside that ecosystem. SpamAssassin is email-only. There is no clean, permissively
licensed, self-hostable "POST content, get a spam score" engine — and nothing OSS targets
the 2026 problem of AI-generated slop flooding short-form UGC. chaffnet is that engine.

## How it works

A hybrid pipeline, no LLM in the request path (sub-10ms, cheap to run):

1. **Heuristic rules** — link ratio, obfuscation, known patterns. Deterministic and explainable.
2. **Reputation lookups** — IP, email/domain, and content-fingerprint reputation.
3. **Small ML classifier** — a trained residual logistic model runs in-process through ONNX
   Runtime. A deterministic heuristic classifier remains available as an evaluation baseline.

The output is a score, never a verdict. You set the threshold.

## Quickstart (self-host)

```bash
cargo run -p chaffnet-server
# in another shell:
curl -s localhost:8080/v1/check \
  -H 'content-type: application/json' \
  -d '{"text":"BUY CHEAP WATCHES https://a.io https://b.io FREE FREE","context":"comment"}'
# => {"spam":0.9...,"slop":0.1...,"reasons":["high_link_ratio","excessive_capitalization"]}
```

Config via env: `CHAFFNET_BIND` (default `0.0.0.0:8080`), `CHAFFNET_DB` (default `chaffnet.redb`).
Machine-readable API description: `GET /llms.txt` and `GET /openapi.json`. Hosted reputation and
feedback configuration is documented in `docs/hosted.md`.

### TypeScript SDK

The dependency-free `chaffnet` package supports browsers, Node.js 20+, and Bun:

```ts
import { ChaffnetClient } from "chaffnet";

const client = new ChaffnetClient({ baseUrl: "http://localhost:8080" });
const result = await client.check({
  text: "A specific comment to classify",
  context: "comment",
});
```

It includes typed single and batch checks, bearer-key authentication, timeouts, cancellation,
structured API errors, and runtime response validation. See `packages/chaffnet/README.md` for the
full client reference.

### MCP server

The `chaffnet-mcp` package exposes the read-only `check_content` tool over stdio:

```json
{
  "mcpServers": {
    "chaffnet": {
      "command": "npx",
      "args": ["-y", "chaffnet-mcp"],
      "env": { "CHAFFNET_BASE_URL": "http://localhost:8080" }
    }
  }
}
```

Set `CHAFFNET_API_KEY` for hosted mode. The tool returns structured spam and slop probabilities plus
reason codes; callers retain control of moderation thresholds. See `docs/mcp.md` for the complete
contract and security boundary.

### Container

The published image is multi-architecture (`linux/amd64` and `linux/arm64`), runs as a non-root
distroless user, and persists its embedded redb store under `/data`:

```bash
docker run --rm -p 8080:8080 \
  --mount source=chaffnet-data,target=/data \
  ghcr.io/iuliandita/chaffnet:edge
```

`edge` tracks `main`; pin the registry digest for production deployments. Version tags publish
when a `v*` Git tag is created. Images include an OCI SBOM and BuildKit provenance attestations.
The image health check calls `GET /healthz` through the server binary itself.

## Two ways to run it

- **Self-host (free, Apache-2.0):** the full engine with rules, a bundled model, and local
  reputation. Runs anywhere.
- **Hosted API (paid, per-check):** the same engine plus the **chaffnet reputation network** — a
  live, cross-customer feed of spam signals (shared as irreversible fingerprints, never raw
  content or IPs) that a single self-hosted instance cannot reproduce. This network is the reason
  the hosted tier exists, and it is the only thing the paid tier adds.

The hosted service mode is implemented for single-instance alpha deployments. It authenticates
all `/v1/*` calls, accepts idempotent spam/ham feedback, and requires consensus from three distinct
tenants before a keyed content fingerprint influences classification. It never persists raw
content or identifiers. Billing, anonymous signup, and horizontal storage remain separate work.

## Agent-native

- Official **MCP server** - agents call `check_content` as a native read-only tool.
- **Programmatic API-key issuance** — agents self-onboard, no dashboard step.
- `/llms.txt`, a single-endpoint OpenAPI spec, and copy-paste `curl` examples.
- **Batch endpoint** for processing queues.

## Evaluation

The evaluation harness scores normalized JSONL corpora through the same Rust engine used by the
server. Fetch the initial CC BY 4.0 spam corpora and evaluate them locally:

```bash
python scripts/fetch_eval_data.py
cargo run -p chaffnet-eval -- evaluate --input data/eval/uci-sms-spam.jsonl
cargo run -p chaffnet-eval -- evaluate --input data/eval/uci-youtube-spam.jsonl
# compare against the pre-model baseline:
cargo run -p chaffnet-eval -- evaluate \
  --input data/eval/uci-sms-spam.jsonl --classifier baseline
```

Downloaded data is gitignored. Canonical URLs, SHA-256 checksums, licenses, citations, and
normalization rules live in `eval/sources.json`. CI uses a small synthetic fixture as a regression
guard; it is not a product-accuracy claim. Slop metrics remain empty until a defensible labeled
short-form corpus is added.

### Spam model

`models/spam-residual-v1.onnx` is the default spam classifier. It consumes seven features derived
by the Rust engine and contributes a residual log-odds term alongside deterministic rules and
reputation. The untouched, deterministic test partition contains 1,241 records grouped by text
hash to prevent duplicate leakage. Against that partition, the model improved average precision
from 0.475 to 0.686 and Brier score from 0.251 to 0.148 relative to the baseline.

The model metadata, source checksums, split policy, tool versions, selected regularization, metrics,
and Rust/Python parity vectors are recorded in `models/spam-residual-v1.json`. Reproduce it after
fetching the source corpora:

```bash
cargo run -p chaffnet-eval -- export-spam-features \
  --input data/eval/uci-sms-spam.jsonl --source uci-sms-spam \
  --output data/eval/uci-sms-spam-features.jsonl
cargo run -p chaffnet-eval -- export-spam-features \
  --input data/eval/uci-youtube-spam.jsonl --source uci-youtube-spam \
  --output data/eval/uci-youtube-spam-features.jsonl
uv run --frozen --group model python scripts/train_spam_model.py \
  --input data/eval/uci-sms-spam-features.jsonl \
  --input data/eval/uci-youtube-spam-features.jsonl
```

## Reputation snapshot

Self-hosted builds embed a normalized snapshot of the public Disposable Email Domains list. The
current snapshot contains 7,978 domains; provider subdomains are matched without sending email
addresses or domains over the network. Source URL, license, retrieval time, response SHA-256, and
row count are recorded in `reputation/snapshot.json`.

Refresh the snapshot explicitly, then review the source and generated-data diff:

```bash
uv run --frozen python scripts/fetch_reputation_data.py
git diff -- reputation/ crates/chaffnet-core/data/
```

Container builds are offline with respect to reputation data. No IP blocklist is bundled: mapping
general malware infrastructure or single-IP indicators onto chaffnet's privacy-preserving `/24`
buckets would create unacceptable false positives. A future IP source must be spam-specific and
validated before it becomes a default seed.

## Status

Early alpha. The trained spam classifier, self-hosted HTTP server, hosted reputation feedback,
public reputation snapshot, GHCR container distribution, TypeScript SDK, and MCP server are
working. Programmatic API-key issuance, horizontal hosted storage, and a defensible slop model
remain planned.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and pull request checks. Report
vulnerabilities through the private process in [SECURITY.md](SECURITY.md), not through a public
issue.

## License

Apache-2.0. The moat is the reputation network (data), not the engine (code) — so the code is
free to take, fork, and self-host.
