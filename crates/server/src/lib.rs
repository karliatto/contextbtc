mod rpc;
mod tools;

use contextvm_sdk::signer;
use contextvm_sdk::transport::server::{NostrServerTransport, NostrServerTransportConfig};
use rmcp::ServiceExt;

use tools::BitcoinRpcNostrServer;

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

    let transport = NostrServerTransport::new(
        signer,
        NostrServerTransportConfig::default()
            .with_relay_urls(vec![
                "ws://localhost:10547".to_string(),
                // "wss://relay.contextvm.org".to_string(),
                // "wss://nos.lol".to_string(),
            ])
            .with_announced_server(false)
            .with_allowed_public_keys(allowed),
    )
    .await?;

    let service = BitcoinRpcNostrServer::new().serve(transport).await?;
    println!("Server ready. Press Ctrl+C to stop.");
    service.waiting().await?;
    Ok(())
}
