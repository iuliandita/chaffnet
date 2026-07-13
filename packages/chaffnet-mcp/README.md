# chaffnet MCP server

Official stdio MCP server for the chaffnet content classification API. It exposes one read-only
tool, `check_content`, for self-hosted and hosted Chaffnet deployments.

## Run

```bash
CHAFFNET_BASE_URL=http://localhost:8080 npx -y chaffnet-mcp
```

Set `CHAFFNET_API_KEY` for hosted mode. `CHAFFNET_TIMEOUT_MS` optionally overrides the SDK's
10-second request timeout with a positive integer number of milliseconds.

Example MCP client configuration:

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

For hosted mode, inject `CHAFFNET_API_KEY` through the client's environment or secret-management
mechanism. Do not commit it to MCP configuration files.

## Tool

`check_content` accepts:

- `text` (required): non-empty content, up to 65,536 UTF-8 bytes.
- `context`: `comment`, `review`, `forum`, or `other`.
- `author_ip`, `author_email`, and `author_name`: optional reputation signals.

It returns structured and JSON text output with `spam` and `slop` probabilities from 0 to 1 plus
machine-readable `reasons`. The caller chooses moderation thresholds; the tool does not make a
publish or reject decision.

The MCP process does not persist content. Self-hosted Chaffnet may store local reputation signals;
hosted persistence and privacy boundaries are documented in `docs/hosted.md` in the repository.

## From a source checkout

```bash
bun install --frozen-lockfile
bun run build:mcp
CHAFFNET_BASE_URL=http://localhost:8080 node packages/chaffnet-mcp/dist/main.js
```
