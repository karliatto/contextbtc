mod client;
mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load variables from a local `.env` file if present. Real environment
    // variables always take precedence and a missing file is not an error.
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage:");
        println!("  cargo run -- server");
        println!("  cargo run -- client <server_pubkey>");
        return Ok(());
    }

    match args[1].as_str() {
        "server" => server::run_server().await,
        "client" => {
            let pubkey = args.get(2).expect("Missing server pubkey");
            client::run_client(pubkey.to_string()).await
        }
        _ => {
            println!("Invalid command");
            Ok(())
        }
    }
}
