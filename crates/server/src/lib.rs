mod rpc;
mod tools;

use contextvm_sdk::signer;
use contextvm_sdk::transport::server::{NostrServerTransport, NostrServerTransportConfig};
use rmcp::ServiceExt;

use tools::BitcoinRpcNostrServer;

/// Nostr relay URLs to connect to, read from the comma-separated
/// `NOSTR_RELAY_URLS` env var. Falls back to a local relay when unset/empty.
fn relay_urls_from_env() -> Vec<String> {
    let urls: Vec<String> = std::env::var("NOSTR_RELAY_URLS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    if urls.is_empty() {
        vec!["ws://localhost:10547".to_string()]
    } else {
        urls
    }
}

/// Run the ContextBTC MCP server until it is shut down.
pub async fn run() -> anyhow::Result<()> {
    let signer = match std::env::var("SERVER_NOSTR_SECRET_KEY") {
        Ok(sk) => signer::from_sk(&sk)?,
        Err(_) => {
            eprintln!(
                "WARNING: SERVER_NOSTR_SECRET_KEY not set; generating an ephemeral key \
                 (identity will change on every restart)."
            );
            signer::generate()
        }
    };
    let pubkey = signer.public_key().to_hex();
    println!("Public key: {pubkey}");

    // Empty `ALLOWED_CLIENT_PUBKEYS` means allow all clients.
    let allowed: Vec<String> = std::env::var("ALLOWED_CLIENT_PUBKEYS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    let relay_urls = relay_urls_from_env();

    let transport = NostrServerTransport::new(
        signer,
        NostrServerTransportConfig::default()
            .with_relay_urls(relay_urls)
            .with_announced_server(false)
            .with_allowed_public_keys(allowed),
    )
    .await?;

    let service = BitcoinRpcNostrServer::new().serve(transport).await?;
    println!("Server ready. Press Ctrl+C to stop.");
    service.waiting().await?;
    Ok(())
}
