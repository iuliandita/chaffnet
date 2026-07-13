# chaffnet TypeScript SDK

Typed, dependency-free client for self-hosted and hosted chaffnet APIs. It runs in modern browsers,
Node.js 20+, and Bun.

## Install

```bash
bun add chaffnet
```

## Use

```ts
import { ChaffnetClient } from "chaffnet";

const chaffnet = new ChaffnetClient({
  baseUrl: "http://localhost:8080",
});

const assessment = await chaffnet.check({
  text: "A specific comment to classify",
  context: "comment",
  author_email: "writer@example.com",
});

if (assessment.spam >= 0.8) {
  // Hold the content for moderation.
}
```

Hosted clients can pass an API key. The SDK sends it as a bearer credential:

```ts
const chaffnet = new ChaffnetClient({
  baseUrl: "https://your-chaffnet-api.example",
  apiKey: process.env.CHAFFNET_API_KEY,
});
```

Batch checks preserve input order and accept up to 1,000 items:

```ts
const assessments = await chaffnet.checkBatch([
  { text: "First comment", context: "comment" },
  { text: "Second comment", context: "comment" },
]);
```

Hosted clients can submit confirmed moderation outcomes:

```ts
await chaffnet.submitFeedback(
  { text: "Confirmed campaign text", context: "comment" },
  "spam",
);
```

The hosted service deduplicates votes per tenant and fingerprint. It does not persist the submitted
content or identifiers.

## Errors, timeouts, and cancellation

The default timeout is 10 seconds. Override it per client and pass an `AbortSignal` per request:

```ts
const chaffnet = new ChaffnetClient({
  baseUrl: "http://localhost:8080",
  timeoutMs: 2_000,
});

const controller = new AbortController();
const pending = chaffnet.check(
  { text: "Content", context: "other" },
  { signal: controller.signal },
);
controller.abort();
await pending;
```

- `ChaffnetApiError` exposes the HTTP status, parsed response body, and response headers.
- `ChaffnetProtocolError` indicates a malformed successful response.
- `ChaffnetTimeoutError` exposes the configured timeout.

The SDK does not retry POST requests because the API does not currently define an idempotency
contract. Callers can retry according to their own queue or request policy.
