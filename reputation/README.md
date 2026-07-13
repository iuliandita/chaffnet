# Public reputation data

`sources.json` is the refresh policy. `snapshot.json` records the exact mutable responses used to
generate the committed data files.

The initial source is Disposable Email Domains, published under CC0-1.0 and explicitly available
for commercial use. chaffnet normalizes the UTF-8 list to lowercase ASCII/IDNA domains, rejects
invalid rows, de-duplicates and sorts it, and enforces record-count bounds before replacing the
existing snapshot.

Refresh from the repository root:

```bash
uv run --frozen python scripts/fetch_reputation_data.py
```

Treat refreshes as source changes: review removals, surprising additions, source checksums, and the
license before merging. The runtime and container build never fetch mutable reputation sources.
