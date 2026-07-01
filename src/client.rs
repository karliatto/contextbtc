use contextvm_sdk::signer;
use contextvm_sdk::transport::client::{NostrClientTransport, NostrClientTransportConfig};
use rmcp::{ClientHandler, ServiceExt, model::CallToolRequestParams};

#[derive(Clone, Default)]
struct BitcoinRpcNostrClient;
impl ClientHandler for BitcoinRpcNostrClient {}

pub async fn run_client(server_pubkey: String) -> anyhow::Result<()> {
    let signer = signer::generate();
    println!("Client starting. Target Server: {}", server_pubkey);

    let transport = NostrClientTransport::new(
        signer,
        NostrClientTransportConfig::default()
            .with_relay_urls(vec![
                "wss://relay.contextvm.org".to_string(),
                "wss://nos.lol".to_string(),
            ])
            .with_server_pubkey(server_pubkey),
    )
    .await?;

    let client = BitcoinRpcNostrClient.serve(transport).await?;

    let tools = client.list_all_tools().await?;
    println!("Discovered {} tool(s).", tools.len());

    let arguments = serde_json::from_value(serde_json::json!({
        "message": "Hello from the Rust client!"
    }))?;
    let result = client
        .call_tool(CallToolRequestParams::new("echo").with_arguments(arguments))
        .await?;

    if let Some(content) = result.content.first() {
        if let rmcp::model::RawContent::Text(text) = &content.raw {
            println!("Result: {}", text.text);
        }
    }

    let arguments = serde_json::from_value(serde_json::json!({
        "a": 1,
        "b": 4
    }))?;
    let result = client
        .call_tool(CallToolRequestParams::new("add").with_arguments(arguments))
        .await?;

    if let Some(content) = result.content.first() {
        if let rmcp::model::RawContent::Text(text) = &content.raw {
            println!("Result: {}", text.text);
        }
    }

    client.cancel().await?;
    Ok(())
}
