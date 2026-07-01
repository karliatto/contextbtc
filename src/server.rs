use contextvm_sdk::signer;
use contextvm_sdk::transport::server::{NostrServerTransport, NostrServerTransportConfig};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
};

#[derive(Clone)]
pub struct BitcoinRpcNostrServer {
    tool_router: ToolRouter<Self>,
}

impl BitcoinRpcNostrServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EchoParams {
    message: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct AddParams {
    a: i64,
    b: i64,
}

#[tool_router]
impl BitcoinRpcNostrServer {
    #[tool(description = "Echo a message back unchanged")]
    async fn echo(
        &self,
        Parameters(EchoParams { message }): Parameters<EchoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Echo: {message}"
        ))]))
    }
    #[tool(description = "Add two numbers")]
    async fn add(
        &self,
        Parameters(AddParams { a, b }): Parameters<AddParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            (a + b).to_string(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for BitcoinRpcNostrServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_server_info(
                Implementation::new("rust-echo-server", "0.1.0").with_title("Rust Echo Server"),
            )
    }
}

pub async fn run_server() -> anyhow::Result<()> {
    let signer = signer::generate();
    let pubkey = signer.public_key().to_hex();
    println!("Server starting. Pubkey: {}", pubkey);

    let transport = NostrServerTransport::new(
        signer,
        NostrServerTransportConfig::default()
            .with_relay_urls(vec![
                "wss://relay.contextvm.org".to_string(),
                "wss://nos.lol".to_string(),
            ])
            .with_announced_server(false),
    )
    .await?;

    let service = BitcoinRpcNostrServer::new().serve(transport).await?;
    println!("Server ready. Press Ctrl+C to stop.");
    service.waiting().await?;
    Ok(())
}
