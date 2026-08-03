//! End-to-end test exercising the full stack:
//!
//! ```text
//!   contextbtc-client ──MCP/Nostr──▶ nak serve (relay) ──▶ contextbtc-server ──JSON-RPC──▶ bitcoind (regtest)
//! ```
//!
//! It starts the stack (see [`harness::Stack`]), then runs the real client
//! binary and asserts it received live data from the node.

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use wait_timeout::ChildExt;

mod harness;

use harness::Stack;

#[test]
fn server_client_roundtrip_over_nostr() -> anyhow::Result<()> {
    let stack = Stack::start(&[])?;

    // --- Run the client and assert it got live regtest data -------------------
    let client_bin = escargot::CargoBuild::new()
        .package("contextbtc-client")
        .bin("contextbtc-client")
        .run()?;
    let mut client = client_bin
        .command()
        .arg(&stack.server_pubkey)
        .env("NOSTR_RELAY_URLS", &stack.relay_url)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let client_stdout = client.stdout.take().expect("client stdout piped");
    let (out_tx, out_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut buf = String::new();
        for line in BufReader::new(client_stdout).lines().map_while(Result::ok) {
            buf.push_str(&line);
            buf.push('\n');
        }
        let _ = out_tx.send(buf);
    });

    let status = match client.wait_timeout(Duration::from_secs(30))? {
        Some(status) => status,
        None => {
            client.kill()?;
            client.wait()?;
            panic!("client did not finish within 30s (likely could not reach the server)");
        }
    };
    let output = out_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();

    println!("{}", status.success());
    println!("output {output}");

    assert!(
        status.success(),
        "client exited with failure: {status:?}\n--- client stdout ---\n{output}"
    );
    assert!(
        output.contains("\"chain\":\"regtest\""),
        "client output missing regtest blockchain info:\n{output}"
    );
    assert!(
        output.contains("Block count:"),
        "client output missing block count:\n{output}"
    );

    Ok(())
}
