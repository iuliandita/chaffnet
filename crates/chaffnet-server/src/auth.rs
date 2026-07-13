use axum::http::{header, HeaderMap};
use chaffnet_core::reputation_hosted::TenantId;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

const MIN_API_KEY_BYTES: usize = 32;
const WINDOW: Duration = Duration::from_secs(60);

struct ApiKeyEntry {
    digest: [u8; 32],
    tenant: TenantId,
}

#[derive(Clone, Copy)]
pub struct ApiCredential<'a> {
    pub tenant: &'a str,
    pub api_key: &'a str,
}

struct RateWindow {
    started_at: Instant,
    requests: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiKeyConfigError {
    #[error("hosted mode requires at least one API key")]
    Empty,
    #[error("API keys must contain at least {MIN_API_KEY_BYTES} bytes")]
    TooShort,
    #[error("tenant names must contain 1 to 64 ASCII letters, digits, underscores, or hyphens")]
    InvalidTenant,
    #[error("duplicate tenant name")]
    DuplicateTenant,
    #[error("duplicate API key")]
    DuplicateKey,
    #[error("rate limit must be greater than zero")]
    InvalidRateLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthRejection {
    Unauthorized,
    RateLimited { retry_after_seconds: u64 },
}

pub struct ApiKeyAuth {
    entries: Vec<ApiKeyEntry>,
    requests_per_minute: u32,
    windows: Mutex<HashMap<TenantId, RateWindow>>,
}

impl ApiKeyAuth {
    pub fn new(
        credentials: &[ApiCredential<'_>],
        requests_per_minute: u32,
    ) -> Result<Self, ApiKeyConfigError> {
        if credentials.is_empty() {
            return Err(ApiKeyConfigError::Empty);
        }
        if requests_per_minute == 0 {
            return Err(ApiKeyConfigError::InvalidRateLimit);
        }

        let mut seen_keys = HashSet::new();
        let mut seen_tenants = HashSet::new();
        let mut entries = Vec::with_capacity(credentials.len());
        for credential in credentials {
            let api_key = credential.api_key;
            if api_key.len() < MIN_API_KEY_BYTES {
                return Err(ApiKeyConfigError::TooShort);
            }
            if credential.tenant.is_empty()
                || credential.tenant.len() > 64
                || !credential
                    .tenant
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err(ApiKeyConfigError::InvalidTenant);
            }
            let digest: [u8; 32] = Sha256::digest(api_key.as_bytes()).into();
            if !seen_keys.insert(digest) {
                return Err(ApiKeyConfigError::DuplicateKey);
            }
            let tenant = TenantId::from_tenant_name(credential.tenant.as_bytes());
            if !seen_tenants.insert(tenant) {
                return Err(ApiKeyConfigError::DuplicateTenant);
            }
            entries.push(ApiKeyEntry { digest, tenant });
        }

        Ok(Self {
            entries,
            requests_per_minute,
            windows: Mutex::new(HashMap::new()),
        })
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> Result<TenantId, AuthRejection> {
        let mut authorization_values = headers.get_all(header::AUTHORIZATION).iter();
        let value = authorization_values
            .next()
            .ok_or(AuthRejection::Unauthorized)?;
        if authorization_values.next().is_some() {
            return Err(AuthRejection::Unauthorized);
        }
        let value = value.to_str().map_err(|_| AuthRejection::Unauthorized)?;
        let mut parts = value.split_ascii_whitespace();
        let scheme = parts.next().ok_or(AuthRejection::Unauthorized)?;
        let token = parts.next().ok_or(AuthRejection::Unauthorized)?;
        if !scheme.eq_ignore_ascii_case("bearer") || parts.next().is_some() {
            return Err(AuthRejection::Unauthorized);
        }

        let candidate: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let mut tenant = None;
        for entry in &self.entries {
            if bool::from(candidate.ct_eq(&entry.digest)) {
                tenant = Some(entry.tenant);
            }
        }
        let tenant = tenant.ok_or(AuthRejection::Unauthorized)?;
        self.apply_rate_limit(tenant)?;
        Ok(tenant)
    }

    fn apply_rate_limit(&self, tenant: TenantId) -> Result<(), AuthRejection> {
        let now = Instant::now();
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let window = windows.entry(tenant).or_insert(RateWindow {
            started_at: now,
            requests: 0,
        });
        let elapsed = now.saturating_duration_since(window.started_at);
        if elapsed >= WINDOW {
            window.started_at = now;
            window.requests = 0;
        }
        if window.requests >= self.requests_per_minute {
            return Err(AuthRejection::RateLimited {
                retry_after_seconds: WINDOW.saturating_sub(elapsed).as_secs().max(1),
            });
        }
        window.requests += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    const KEY: &str = "cn_test_abcdefghijklmnopqrstuvwxyz0123456789";

    fn credential<'a>(tenant: &'a str, api_key: &'a str) -> ApiCredential<'a> {
        ApiCredential { tenant, api_key }
    }

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn valid_bearer_key_authenticates() {
        let auth = ApiKeyAuth::new(&[credential("tenant-a", KEY)], 10).unwrap();
        assert!(auth
            .authenticate(&headers(&format!("Bearer {KEY}")))
            .is_ok());
    }

    #[test]
    fn malformed_and_invalid_keys_are_rejected() {
        let auth = ApiKeyAuth::new(&[credential("tenant-a", KEY)], 10).unwrap();
        assert!(matches!(
            auth.authenticate(&HeaderMap::new()),
            Err(AuthRejection::Unauthorized)
        ));
        assert!(matches!(
            auth.authenticate(&headers("Basic abc")),
            Err(AuthRejection::Unauthorized)
        ));
        assert!(matches!(
            auth.authenticate(&headers("Bearer wrong-key-with-enough-characters-12345")),
            Err(AuthRejection::Unauthorized)
        ));
    }

    #[test]
    fn rate_limit_is_enforced_after_authentication() {
        let auth = ApiKeyAuth::new(&[credential("tenant-a", KEY)], 1).unwrap();
        let headers = headers(&format!("Bearer {KEY}"));
        assert!(auth.authenticate(&headers).is_ok());
        assert!(matches!(
            auth.authenticate(&headers),
            Err(AuthRejection::RateLimited { .. })
        ));
    }

    #[test]
    fn duplicate_tenants_are_rejected_even_when_keys_differ() {
        let other_key = "cn_test_other_abcdefghijklmnopqrstuvwxyz0123456789";
        assert!(matches!(
            ApiKeyAuth::new(
                &[
                    credential("tenant-a", KEY),
                    credential("tenant-a", other_key),
                ],
                10,
            ),
            Err(ApiKeyConfigError::DuplicateTenant)
        ));
    }

    #[test]
    fn duplicate_keys_and_invalid_tenant_names_are_rejected() {
        assert!(matches!(
            ApiKeyAuth::new(
                &[credential("tenant-a", KEY), credential("tenant-b", KEY),],
                10,
            ),
            Err(ApiKeyConfigError::DuplicateKey)
        ));
        assert!(matches!(
            ApiKeyAuth::new(&[credential("tenant with spaces", KEY)], 10),
            Err(ApiKeyConfigError::InvalidTenant)
        ));
    }
}
