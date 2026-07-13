//! Pure-Rust embedded reputation store (redb). Seeds from bundled public data so
//! it is useful with zero feedback (cold-start mitigation R2). The committed seed
//! files are small samples; a package-time fetch pulls the full public lists.

use crate::fingerprint::hamming;
use crate::reputation::ReputationStore;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;

const DISPOSABLE: TableDefinition<&str, ()> = TableDefinition::new("disposable_domains");
const IP_BUCKETS: TableDefinition<u64, f32> = TableDefinition::new("ip_buckets");
const DOMAIN_SCORES: TableDefinition<&str, f32> = TableDefinition::new("domain_scores");
const FINGERPRINTS: TableDefinition<u64, f32> = TableDefinition::new("fingerprints");

const SEED_DISPOSABLE: &str = include_str!("../data/disposable-email-domains.txt");
const SEED_IP_RANGES: &str = include_str!("../data/spam-ip-ranges.txt");

/// Wraps any redb error. The payload is boxed because redb's error types are
/// large (~160 bytes); boxing keeps `Result<_, StoreError>` small enough to
/// satisfy `clippy::result_large_err` while preserving the public contract
/// (`open_seeded`/`record_spam_fingerprint` return `Result<_, StoreError>`).
#[derive(Debug, thiserror::Error)]
#[error("database error: {0}")]
pub struct StoreError(Box<redb::Error>);

// redb exposes `From<SubError> for redb::Error` for every operation error, so we
// funnel all of them through `redb::Error` into the single boxed variant. Each
// `?` in this module relies on one of these conversions.
macro_rules! store_error_from {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for StoreError {
                fn from(e: $ty) -> Self {
                    StoreError(Box::new(e.into()))
                }
            }
        )*
    };
}

impl From<redb::Error> for StoreError {
    fn from(e: redb::Error) -> Self {
        StoreError(Box::new(e))
    }
}

store_error_from!(
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
    redb::DatabaseError,
);

pub struct LocalStore {
    db: Database,
}

impl LocalStore {
    /// Open (creating if needed) and ensure the store is seeded from bundled data.
    pub fn open_seeded(path: &Path) -> Result<Self, StoreError> {
        let db = Database::create(path)?;
        let store = Self { db };
        store.seed()?;
        Ok(store)
    }

    fn seed(&self) -> Result<(), StoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(DISPOSABLE)?;
            for line in SEED_DISPOSABLE.lines() {
                let d = line.trim();
                if !d.is_empty() && !d.starts_with('#') {
                    t.insert(d, ())?;
                }
            }
        }
        {
            let mut t = wtx.open_table(IP_BUCKETS)?;
            for line in SEED_IP_RANGES.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                if let (Some(prefix), Some(score)) = (parts.next(), parts.next()) {
                    let octets: Vec<&str> = prefix.split('.').collect();
                    if octets.len() == 3 {
                        if let (Ok(a), Ok(b), Ok(c), Ok(s)) = (
                            octets[0].parse::<u8>(),
                            octets[1].parse::<u8>(),
                            octets[2].parse::<u8>(),
                            score.parse::<f32>(),
                        ) {
                            // Use the shared helper so stored buckets match queries.
                            t.insert(crate::fingerprint::ip_bucket_v4([a, b, c]), s)?;
                        }
                    }
                }
            }
        }
        wtx.commit()?;
        Ok(())
    }

    /// Record that a content fingerprint was confirmed spam (feedback endpoint).
    pub fn record_spam_fingerprint(&self, fp: u64) -> Result<(), StoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(FINGERPRINTS)?;
            t.insert(fp, 1.0f32)?;
        }
        wtx.commit()?;
        Ok(())
    }
}

impl ReputationStore for LocalStore {
    fn ip_bucket_score(&self, bucket: u64) -> Option<f32> {
        let rtx = self.db.begin_read().ok()?;
        let t = rtx.open_table(IP_BUCKETS).ok()?;
        t.get(bucket).ok()?.map(|v| v.value())
    }

    fn email_domain_score(&self, domain: &str) -> Option<f32> {
        let rtx = self.db.begin_read().ok()?;
        let t = rtx.open_table(DOMAIN_SCORES).ok()?;
        t.get(domain).ok()?.map(|v| v.value())
    }

    fn fingerprint_score(&self, fp: u64) -> Option<f32> {
        let rtx = self.db.begin_read().ok()?;
        let t = rtx.open_table(FINGERPRINTS).ok()?;
        // Exact hit first, then near-duplicate scan (Hamming <= 3).
        if let Some(v) = t.get(fp).ok()? {
            return Some(v.value());
        }
        let mut best: Option<f32> = None;
        for row in t.iter().ok()? {
            let (k, v) = row.ok()?;
            if hamming(k.value(), fp) <= 3 {
                let s = v.value();
                best = Some(best.map_or(s, |b| b.max(s)));
            }
        }
        best
    }

    fn is_disposable_domain(&self, domain: &str) -> bool {
        let lower = domain.trim_end_matches('.').to_lowercase();
        (|| {
            let rtx = self.db.begin_read().ok()?;
            let t = rtx.open_table(DISPOSABLE).ok()?;
            let mut candidate = lower.as_str();
            while candidate.contains('.') {
                if t.get(candidate).ok()?.is_some() {
                    return Some(true);
                }
                candidate = candidate.split_once('.')?.1;
            }
            Some(false)
        })()
        .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reputation::ReputationStore;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "chaffnet-test-{}-{}.redb",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn seeds_disposable_domains_from_bundled_data() {
        let path = temp_path("seed");
        let store = LocalStore::open_seeded(&path).unwrap();
        assert!(store.is_disposable_domain("mailinator.com"));
        assert!(!store.is_disposable_domain("gmail.com"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn disposable_domain_seed_matches_provider_subdomains() {
        let path = temp_path("seed-subdomain");
        let store = LocalStore::open_seeded(&path).unwrap();
        assert!(store.is_disposable_domain("inbox.mailinator.com"));
        assert!(!store.is_disposable_domain("mailinator.com.example.org"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn records_and_reads_back_fingerprint_feedback() {
        let path = temp_path("feedback");
        let store = LocalStore::open_seeded(&path).unwrap();
        let fp = crate::fingerprint::simhash("known spam phrase buy now cheap");
        store.record_spam_fingerprint(fp).unwrap();
        assert!(store.fingerprint_score(fp).unwrap() > 0.5);
        std::fs::remove_file(&path).ok();
    }
}
