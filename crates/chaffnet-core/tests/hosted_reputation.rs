use chaffnet_core::fingerprint::{network_fingerprint, NetworkKey};
use chaffnet_core::reputation::ReputationStore;
use chaffnet_core::reputation_hosted::{FeedbackVerdict, HostedStore, TenantId};
use std::sync::atomic::{AtomicU64, Ordering};

static DB_COUNTER: AtomicU64 = AtomicU64::new(0);
const NETWORK_SECRET: &[u8] = b"test-network-secret-with-at-least-32-bytes";

fn paths(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let n = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir();
    let local = root.join(format!(
        "chaffnet-hosted-{name}-{}-{n}-local.redb",
        std::process::id()
    ));
    let network = root.join(format!(
        "chaffnet-hosted-{name}-{}-{n}-network.redb",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&local);
    let _ = std::fs::remove_file(&network);
    (local, network)
}

fn tenant(suffix: char) -> TenantId {
    TenantId::from_tenant_name(format!("tenant-{suffix}").as_bytes())
}

#[test]
fn network_fingerprints_are_keyed_and_stable() {
    let a = NetworkKey::derive(NETWORK_SECRET);
    let b = NetworkKey::derive(b"another-network-secret-with-32-bytes-minimum");
    let local = 0x1122_3344_5566_7788;

    assert_eq!(
        network_fingerprint(local, &a),
        network_fingerprint(local, &a)
    );
    assert_ne!(network_fingerprint(local, &a), local);
    assert_ne!(
        network_fingerprint(local, &a),
        network_fingerprint(local, &b)
    );
}

#[test]
fn hosted_store_rejects_short_network_secrets() {
    let (local, network) = paths("short-secret");
    assert!(HostedStore::open(&local, &network, b"too-short").is_err());
}

#[test]
fn one_tenant_cannot_create_network_reputation() {
    let (local, network) = paths("one-tenant");
    let store = HostedStore::open(&local, &network, NETWORK_SECRET).unwrap();
    let fingerprint = 42;

    let first = store
        .record_feedback(tenant('a'), fingerprint, FeedbackVerdict::Spam)
        .unwrap();
    let duplicate = store
        .record_feedback(tenant('a'), fingerprint, FeedbackVerdict::Spam)
        .unwrap();

    assert!(first.changed);
    assert!(!duplicate.changed);
    assert_eq!(duplicate.distinct_tenants, 1);
    assert_eq!(store.fingerprint_score(fingerprint), None);
}

#[test]
fn three_tenants_create_reputation_and_can_change_votes() {
    let (local, network) = paths("consensus");
    let store = HostedStore::open(&local, &network, NETWORK_SECRET).unwrap();
    let fingerprint = 99;

    for id in [tenant('a'), tenant('b'), tenant('c')] {
        store
            .record_feedback(id, fingerprint, FeedbackVerdict::Spam)
            .unwrap();
    }
    assert_eq!(store.fingerprint_score(fingerprint), Some(1.0));

    let changed = store
        .record_feedback(tenant('a'), fingerprint, FeedbackVerdict::Ham)
        .unwrap();
    assert!(changed.changed);
    assert_eq!(changed.distinct_tenants, 3);
    assert!((store.fingerprint_score(fingerprint).unwrap() - (2.0 / 3.0)).abs() < 1e-6);
}

#[test]
fn network_votes_survive_reopen() {
    let (local, network) = paths("reopen");
    let fingerprint = 1234;
    {
        let store = HostedStore::open(&local, &network, NETWORK_SECRET).unwrap();
        for id in [tenant('a'), tenant('b'), tenant('c')] {
            store
                .record_feedback(id, fingerprint, FeedbackVerdict::Spam)
                .unwrap();
        }
    }

    let reopened = HostedStore::open(&local, &network, NETWORK_SECRET).unwrap();
    assert_eq!(reopened.fingerprint_score(fingerprint), Some(1.0));
}
