# Hosted reputation mode

Hosted mode runs the normal classifier with bearer-key authentication and a shared feedback store.
It is intended for a single service instance in the current alpha. The embedded redb database is
durable, but it is not a horizontally replicated service.

## Configure

Generate durable, high-entropy values with a secret manager or a local cryptographic generator:

```sh
openssl rand -hex 32
```

Set the generated values through your process supervisor or secret manager:

```text
CHAFFNET_MODE=hosted
CHAFFNET_NETWORK_SECRET=<durable 32-byte-or-longer secret>
CHAFFNET_API_KEYS=tenant-a=<32-byte-or-longer key>,tenant-b=<32-byte-or-longer key>
CHAFFNET_NETWORK_DB=/data/chaffnet-network.redb
CHAFFNET_RATE_LIMIT_PER_MINUTE=600
```

Tenant names may contain 1 to 64 ASCII letters, digits, underscores, or hyphens. They are stable
voting identities: rotate a tenant's key without changing its name so that key rotation cannot
create another vote. The server stores neither tenant names nor API keys.

`CHAFFNET_NETWORK_SECRET` keys every shared fingerprint. Back it up separately and restrict access.
Changing it makes existing network reputation unreachable, so treat replacement as a new network
or an explicit migration.

The normal `CHAFFNET_BIND` and `CHAFFNET_DB` settings still apply. The container image defaults both
databases to `/data`; mount that directory on durable storage.

## Call the API

All `/v1/*` routes require a configured bearer key in hosted mode. Health and machine-readable docs
remain public.

```sh
curl -s http://localhost:8080/v1/check \
  -H "authorization: Bearer $CHAFFNET_API_KEY" \
  -H 'content-type: application/json' \
  -d '{"text":"content to classify","context":"comment"}'
```

Submit a confirmed moderation outcome with the same content shape:

```sh
curl -s http://localhost:8080/v1/feedback \
  -H "authorization: Bearer $CHAFFNET_API_KEY" \
  -H 'content-type: application/json' \
  -d '{"content":{"text":"confirmed campaign text","context":"comment"},"verdict":"spam"}'
```

Feedback is idempotent per tenant and fingerprint. A changed verdict replaces that tenant's prior
vote. Network reputation affects checks only after three distinct tenants have voted.

## Privacy and operations

The network database contains only:

- deployment-keyed exact-match content fingerprints;
- truncated hashes of stable tenant names;
- per-tenant verdict bytes and aggregate counts.

It does not contain content, IP addresses, emails, domains, tenant names, API keys, or unsalted
fingerprints. The alpha deliberately does not infer shared IP or email-domain reputation from a
generic spam verdict because that would create unsafe false positives for NAT ranges and shared
mail providers.

Terminate TLS and apply source-level volumetric limits at a trusted reverse proxy. The application
also enforces a fixed-window per-tenant request limit and returns `429` with `Retry-After`.

For backup, stop writes or stop the service, then back up both redb files and the network secret.
Test restoring all three together. API keys can be restored from the external secret manager.
