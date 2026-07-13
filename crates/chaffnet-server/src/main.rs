use chaffnet_server::app::{build_app, AppState};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let db_path = std::env::var("CHAFFNET_DB").unwrap_or_else(|_| "chaffnet.redb".to_string());
    let bind = std::env::var("CHAFFNET_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        if let Err(error) =
            chaffnet_server::healthcheck::check_endpoint(&bind, std::time::Duration::from_secs(3))
        {
            eprintln!("healthcheck failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    let state = match std::env::var("CHAFFNET_MODE").as_deref() {
        Ok("hosted") => {
            let network_db = std::env::var("CHAFFNET_NETWORK_DB")
                .unwrap_or_else(|_| "chaffnet-network.redb".to_string());
            let network_secret = std::env::var("CHAFFNET_NETWORK_SECRET")
                .expect("CHAFFNET_NETWORK_SECRET is required in hosted mode");
            let api_keys = std::env::var("CHAFFNET_API_KEYS")
                .expect("CHAFFNET_API_KEYS is required in hosted mode");
            let api_keys: Vec<chaffnet_server::auth::ApiCredential<'_>> = api_keys
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(|entry| {
                    let (tenant, api_key) = entry
                        .split_once('=')
                        .expect("CHAFFNET_API_KEYS entries must use tenant=key");
                    chaffnet_server::auth::ApiCredential {
                        tenant: tenant.trim(),
                        api_key: api_key.trim(),
                    }
                })
                .collect();
            let requests_per_minute = std::env::var("CHAFFNET_RATE_LIMIT_PER_MINUTE")
                .map(|value| {
                    value
                        .parse::<u32>()
                        .expect("CHAFFNET_RATE_LIMIT_PER_MINUTE must be an integer")
                })
                .unwrap_or(600);
            AppState::new_hosted(
                std::path::Path::new(&db_path),
                std::path::Path::new(&network_db),
                network_secret.as_bytes(),
                &api_keys,
                requests_per_minute,
            )
        }
        Ok("self-hosted") | Ok("local") | Err(std::env::VarError::NotPresent) => {
            AppState::new_local(std::path::Path::new(&db_path))
        }
        Ok(mode) => panic!("unsupported CHAFFNET_MODE {mode:?}"),
        Err(error) => panic!("failed to read CHAFFNET_MODE: {error}"),
    }
    .expect("failed to initialize application state");
    let app = build_app(Arc::new(state));

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("bind failed");
    tracing::info!("chaffnet listening on {bind}");
    axum::serve(listener, app).await.expect("server error");
}
