use sha2::{Digest, Sha256};
use siphasher::sip::SipHasher13;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;

// Fixed keys so hashing is stable across Rust/std toolchain versions. std's
// `DefaultHasher` (SipHash-1-3) is explicitly NOT guaranteed stable across
// releases; these fingerprints are persisted (redb) and compared over time, so a
// toolchain bump must not silently change them.
const SIP_K0: u64 = 0x9E37_79B9_7F4A_7C15;
const SIP_K1: u64 = 0xF39C_BB7E_EEA7_C457;

fn hash64<T: Hash>(t: &T) -> u64 {
    hash64_with_keys(t, SIP_K0, SIP_K1)
}

fn hash64_with_keys<T: Hash>(t: &T, k0: u64, k1: u64) -> u64 {
    let mut h = SipHasher13::new_with_keys(k0, k1);
    t.hash(&mut h);
    h.finish()
}

/// Deployment-specific key for hosted network fingerprints.
#[derive(Clone)]
pub struct NetworkKey {
    k0: u64,
    k1: u64,
}

impl NetworkKey {
    /// Derive a SipHash key from a durable, high-entropy deployment secret.
    pub fn derive(secret: &[u8]) -> Self {
        let digest = Sha256::digest(secret);
        let k0 = u64::from_le_bytes(digest[0..8].try_into().expect("fixed digest slice"));
        let k1 = u64::from_le_bytes(digest[8..16].try_into().expect("fixed digest slice"));
        Self { k0, k1 }
    }
}

/// Convert a local SimHash to a deployment-keyed exact-match fingerprint.
///
/// Keying prevents tenants from probing the shared store with known unsalted
/// SimHashes. It deliberately gives up Hamming-distance lookups: the hosted
/// alpha only shares exact content fingerprints.
pub fn network_fingerprint(local_fingerprint: u64, key: &NetworkKey) -> u64 {
    hash64_with_keys(&local_fingerprint, key.k0, key.k1)
}

/// 64-bit SimHash over whitespace token 2-shingles. Irreversible: only the hash
/// is ever persisted or shared (privacy constraint R3).
///
/// Hashing is stable across Rust toolchains (fixed SipHash-1-3 keys), so a
/// compiler upgrade will not desync persisted fingerprints.
///
/// The hash is lossy but not keyed, which is appropriate for local lookups. The
/// hosted store converts it to a deployment-keyed exact-match fingerprint before
/// persistence so tenants cannot probe the shared database with known local
/// SimHashes.
pub fn simhash(text: &str) -> u64 {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return 0;
    }
    let mut columns = [0i32; 64];
    // Unigrams and adjacent bigrams as features.
    let mut features: Vec<u64> = tokens.iter().map(|t| hash64(&t.to_lowercase())).collect();
    for w in tokens.windows(2) {
        features.push(hash64(&format!(
            "{} {}",
            w[0].to_lowercase(),
            w[1].to_lowercase()
        )));
    }
    for f in features {
        for (i, col) in columns.iter_mut().enumerate() {
            if (f >> i) & 1 == 1 {
                *col += 1;
            } else {
                *col -= 1;
            }
        }
    }
    let mut out = 0u64;
    for (i, col) in columns.iter().enumerate() {
        if *col > 0 {
            out |= 1 << i;
        }
    }
    out
}

/// Hamming distance between two SimHashes (0..=64).
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Hash a /24 IPv4 prefix (first three octets) into a bucket. This is the single
/// source of truth for the V4 bucketing so query-time and seed-loading paths
/// (Task 10) cannot drift apart.
pub fn ip_bucket_v4(octets: [u8; 3]) -> u64 {
    hash64(&octets)
}

/// Collapse an IP to a network bucket and hash it. IPv4 -> /24, IPv6 -> /48.
/// The raw address is never stored (privacy constraint R3).
///
/// Hashing is stable across Rust toolchains (fixed SipHash-1-3 keys), so
/// persisted buckets survive a compiler upgrade.
///
/// Buckets are lossy but not keyed, which is fine for single-tenant self-hosting.
/// Hosted feedback does not persist or share IP buckets because generic content
/// verdicts are too weak to assign reputation to shared NAT ranges.
pub fn ip_bucket(ip: IpAddr) -> u64 {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            ip_bucket_v4([o[0], o[1], o[2]])
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            hash64(&[s[0], s[1], s[2]])
        }
    }
}

/// Extract and normalize the domain part of an email address: the substring after
/// the final `@`, trimmed and lowercased. Returns `None` if there is no `@`, if
/// either side of the final `@` is empty, or if the domain has no `.`. Multiple
/// `@` are tolerated (the last one wins). Only the domain is ever used for
/// reputation, never the local part.
pub fn email_domain(email: &str) -> Option<String> {
    let parts: Vec<&str> = email.rsplitn(2, '@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return None;
    }
    let domain = parts[0].trim().to_lowercase();
    if domain.contains('@') || !domain.contains('.') {
        return None;
    }
    Some(domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn simhash_is_stable_and_near_for_similar_text() {
        let a = simhash("buy cheap watches at example dot com now");
        let b = simhash("buy cheap watches at example dot com today");
        let c = simhash("the quarterly financial report is attached");
        assert_eq!(a, simhash("buy cheap watches at example dot com now")); // deterministic
        assert!(hamming(a, b) < hamming(a, c)); // similar text -> smaller distance
    }

    #[test]
    fn ipv4_bucket_collapses_host_octet() {
        let a = ip_bucket(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)));
        let b = ip_bucket(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 200)));
        let d = ip_bucket(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 5)));
        assert_eq!(a, b); // same /24 -> same bucket
        assert_ne!(a, d);
    }

    #[test]
    fn ipv6_bucket_collapses_to_48() {
        let a = ip_bucket(IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0x1, 0xaaaa, 0, 0, 0, 1,
        )));
        let b = ip_bucket(IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0x1, 0xbbbb, 0, 0, 0, 2,
        )));
        assert_eq!(a, b); // same /48
    }

    #[test]
    fn email_domain_is_lowercased_and_extracted() {
        assert_eq!(
            email_domain("User@Example.COM").as_deref(),
            Some("example.com")
        );
        assert_eq!(email_domain("not-an-email"), None);
    }
}
