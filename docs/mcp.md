# MCP server

The official `chaffnet-mcp` package exposes Chaffnet classification as a local stdio MCP server.
The MCP client starts the process and communicates over stdin/stdout; the process calls the
configured Chaffnet HTTP API.

## Configuration

| Variable | Required | Description |
| --- | --- | --- |
| `CHAFFNET_BASE_URL` | yes | Self-hosted or hosted Chaffnet API root |
| `CHAFFNET_API_KEY` | hosted only | Bearer key sent to protected hosted routes |
| `CHAFFNET_TIMEOUT_MS` | no | Positive integer request timeout; defaults to 10,000 ms |

Example generic MCP client configuration:

```json
{
  "mcpServers": {
    "chaffnet": {
      "command": "npx",
      "args": ["-y", "chaffnet-mcp"],
      "env": {
        "CHAFFNET_BASE_URL": "http://localhost:8080"
      }
    }
  }
}
```

Hosted users add `CHAFFNET_API_KEY` through their client's environment or secret store. Avoid
placing credentials in tracked configuration.

## `check_content`

The read-only, idempotent tool accepts the same content fields as `POST /v1/check`:

```json
{
  "text": "A comment to classify",
  "context": "comment",
  "author_ip": "203.0.113.10",
  "author_email": "writer@example.com",
  "author_name": "Writer"
}
```

Only `text` is required. Its maximum is 65,536 UTF-8 bytes. `context` is one of `comment`,
`review`, `forum`, or `other`. The result is available as both MCP structured content and compact
JSON text:

```json
{
  "spam": 0.12,
  "slop": 0.08,
  "reasons": []
}
```

Scores are probabilities, not moderation decisions. The caller owns its threshold and policy.

## Security boundary

- Tool input is schema-validated before the API call; content over the server limit is rejected.
- Input is sent only as JSON to the configured fixed base URL. It is never used as a command,
  file path, query, or destination URL.
- Tool failures return stable public messages without upstream response bodies, stack traces, or
  file paths.
- The MCP process stores no content. See `hosted.md` for hosted reputation persistence rules.
- The server intentionally does not expose feedback mutation. Submit confirmed outcomes through
  an integration that can enforce human approval and tenant policy.

## Verify a source checkout

```bash
bun install --frozen-lockfile
bun run typecheck:mcp
bun run test:mcp
bun run pack:mcp
```

The test suite exercises the MCP protocol in memory and through a real Node.js stdio child process.
