use crate::fingerprint::{network_fingerprint, NetworkKey};
use crate::reputation::ReputationStore;
use crate::reputation_local::{LocalStore, StoreError};
use redb::{Database, ReadableTable, TableDefinition};
use sha2::{Digest, Sha256};
use std::path::Path;

const VOTES: TableDefinition<&[u8], u8> = TableDefinition::new("hosted_fingerprint_votes_v1");
const AGGREGATES: TableDefinition<u64, u64> =
    TableDefinition::new("hosted_fingerprint_aggregates_v1");
const MIN_DISTINCT_TENANTS: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum HostedStoreError {
    #[error(transparent)]
    Local(#[from] StoreError),
    #[error("network database error: {0}")]
    Database(Box<redb::Error>),
    #[error("network reputation vote capacity exceeded")]
    VoteCapacity,
    #[error("network reputation vote data is inconsistent")]
    CorruptVote,
    #[error("network secret must contain at least 32 bytes")]
    NetworkSecretTooShort,
}

macro_rules! hosted_error_from {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for HostedStoreError {
                fn from(error: $ty) -> Self {
                    Self::Database(Box::new(error.into()))
                }
            }
        )*
    };
}

impl From<redb::Error> for HostedStoreError {
    fn from(error: redb::Error) -> Self {
        Self::Database(Box::new(error))
    }
}

hosted_error_from!(
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
    redb::DatabaseError,
);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantId([u8; 16]);

impl TenantId {
    /// Derive a stable identifier from a tenant name without retaining it.
    /// API-key rotation therefore cannot create a second voting identity.
    pub fn from_tenant_name(tenant_name: &[u8]) -> Self {
        let digest = Sha256::digest(tenant_name);
        Self(digest[0..16].try_into().expect("fixed digest slice"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FeedbackVerdict {
    Ham = 0,
    Spam = 1,
}

impl FeedbackVerdict {
    fn from_stored(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Ham),
            1 => Some(Self::Spam),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackReceipt {
    pub changed: bool,
    pub distinct_tenants: u32,
}

pub struct HostedStore {
    local: LocalStore,
    network: Database,
    key: NetworkKey,
}

impl HostedStore {
    pub fn open(
        local_path: &Path,
        network_path: &Path,
        network_secret: &[u8],
    ) -> Result<Self, HostedStoreError> {
        if network_secret.len() < 32 {
            return Err(HostedStoreError::NetworkSecretTooShort);
        }
        let local = LocalStore::open_seeded(local_path)?;
        let network = Database::create(network_path)?;
        let store = Self {
            local,
            network,
            key: NetworkKey::derive(network_secret),
        };
        store.initialize()?;
        Ok(store)
    }

    fn initialize(&self) -> Result<(), HostedStoreError> {
        let write = self.network.begin_write()?;
        write.open_table(VOTES)?;
        write.open_table(AGGREGATES)?;
        write.commit()?;
        Ok(())
    }

    pub fn record_feedback(
        &self,
        tenant: TenantId,
        local_fingerprint: u64,
        verdict: FeedbackVerdict,
    ) -> Result<FeedbackReceipt, HostedStoreError> {
        let fingerprint = network_fingerprint(local_fingerprint, &self.key);
        let mut vote_key = [0u8; 24];
        vote_key[..8].copy_from_slice(&fingerprint.to_le_bytes());
        vote_key[8..].copy_from_slice(&tenant.0);

        let write = self.network.begin_write()?;
        let (changed, spam, ham) = {
            let mut votes = write.open_table(VOTES)?;
            let mut aggregates = write.open_table(AGGREGATES)?;
            let previous = match votes.get(vote_key.as_slice())? {
                Some(value) => Some(
                    FeedbackVerdict::from_stored(value.value())
                        .ok_or(HostedStoreError::CorruptVote)?,
                ),
                None => None,
            };
            let packed = aggregates
                .get(fingerprint)?
                .map(|value| value.value())
                .unwrap_or(0);
            let mut spam = (packed >> 32) as u32;
            let mut ham = packed as u32;

            if previous == Some(verdict) {
                (false, spam, ham)
            } else {
                match previous {
                    Some(FeedbackVerdict::Spam) => {
                        spam = spam.checked_sub(1).ok_or(HostedStoreError::CorruptVote)?
                    }
                    Some(FeedbackVerdict::Ham) => {
                        ham = ham.checked_sub(1).ok_or(HostedStoreError::CorruptVote)?
                    }
                    None => {}
                }
                match verdict {
                    FeedbackVerdict::Spam => {
                        spam = spam.checked_add(1).ok_or(HostedStoreError::VoteCapacity)?
                    }
                    FeedbackVerdict::Ham => {
                        ham = ham.checked_add(1).ok_or(HostedStoreError::VoteCapacity)?
                    }
                }
                votes.insert(vote_key.as_slice(), verdict as u8)?;
                aggregates.insert(fingerprint, ((spam as u64) << 32) | ham as u64)?;
                (true, spam, ham)
            }
        };
        let distinct_tenants = spam
            .checked_add(ham)
            .ok_or(HostedStoreError::VoteCapacity)?;
        write.commit()?;
        Ok(FeedbackReceipt {
            changed,
            distinct_tenants,
        })
    }

    fn network_fingerprint_score(&self, local_fingerprint: u64) -> Option<f32> {
        let fingerprint = network_fingerprint(local_fingerprint, &self.key);
        let read = self.network.begin_read().ok()?;
        let aggregates = read.open_table(AGGREGATES).ok()?;
        let packed = aggregates.get(fingerprint).ok()??.value();
        let spam = (packed >> 32) as u32;
        let ham = packed as u32;
        let total = spam.checked_add(ham)?;
        if total < MIN_DISTINCT_TENANTS {
            return None;
        }
        Some(spam as f32 / total as f32)
    }
}

impl ReputationStore for HostedStore {
    fn ip_bucket_score(&self, bucket: u64) -> Option<f32> {
        self.local.ip_bucket_score(bucket)
    }

    fn email_domain_score(&self, domain: &str) -> Option<f32> {
        self.local.email_domain_score(domain)
    }

    fn fingerprint_score(&self, fingerprint: u64) -> Option<f32> {
        match (
            self.local.fingerprint_score(fingerprint),
            self.network_fingerprint_score(fingerprint),
        ) {
            (Some(local), Some(network)) => Some(local.max(network)),
            (Some(score), None) | (None, Some(score)) => Some(score),
            (None, None) => None,
        }
    }

    fn is_disposable_domain(&self, domain: &str) -> bool {
        self.local.is_disposable_domain(domain)
    }
}
