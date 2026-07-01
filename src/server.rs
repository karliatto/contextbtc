use serde_json::{Value, json};
use std::sync::Arc;

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
    rpc: Arc<BitcoinRpc>,
}

impl BitcoinRpcNostrServer {
    pub fn new() -> Self {
        let rpc = BitcoinRpc {
            http: reqwest::Client::new(),
            url: std::env::var("BITCOIN_RPC_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8332".to_string()),
            user: std::env::var("BITCOIN_RPC_USER").unwrap_or_default(),
            password: std::env::var("BITCOIN_RPC_PASSWORD").unwrap_or_default(),
        };
        Self {
            tool_router: Self::tool_router(),
            rpc: Arc::new(rpc),
        }
    }
}
struct BitcoinRpc {
    http: reqwest::Client,
    url: String, // e.g. "http://127.0.0.1:8332"
    user: String,
    password: String,
}

impl BitcoinRpc {
    async fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": "bitcoin-rpc-nostr",
            "method": method,
            "params": params,
        });

        let resp: Value = self
            .http
            .post(&self.url)
            .basic_auth(&self.user, Some(&self.password))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp.get("error").filter(|e| !e.is_null()) {
            anyhow::bail!("bitcoind RPC error: {err}");
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GetBlockParams {
    /// Block hash (hex)
    blockhash: String,
    /// Verbosity: 0=hex, 1=json, 2=json with tx details
    #[serde(default)]
    verbosity: Option<u8>,
}

#[tool_router]
impl BitcoinRpcNostrServer {
    #[tool(description = "Get current blockchain state (height, chain, difficulty, ...)")]
    async fn get_blockchain_info(&self) -> Result<CallToolResult, ErrorData> {
        let result = self
            .rpc
            .call("getblockchaininfo", json!([]))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }

    #[tool(description = "Get a block by hash")]
    async fn get_block(
        &self,
        Parameters(GetBlockParams {
            blockhash,
            verbosity,
        }): Parameters<GetBlockParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = json!([blockhash, verbosity.unwrap_or(1)]);
        let result = self
            .rpc
            .call("getblock", params)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
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
