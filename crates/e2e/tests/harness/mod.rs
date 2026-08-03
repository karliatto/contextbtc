//! Shared plumbing for the end-to-end tests.
//!
//! [`Stack`] brings up the three moving parts every test needs — a local Nostr
//! relay (`nak serve`), a regtest `bitcoind` (via `corepc-node`), and the real
//! `contextbtc-server` binary wired to both — and tears them all down when it
//! is dropped.
//!
//! Tests using it require a `bitcoind` binary (located via `BITCOIND_EXE` or
//! `PATH`) and `nak` on `PATH`. The Nix devShell provides both, so run them
//! with `nix develop --command cargo test`.

// Each test binary compiles its own copy of this module, and none of them use
// all of it.
#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Fixed server identity so the run is deterministic and warning-free (no
/// ephemeral key). This is a throwaway test key, not a secret.
const SERVER_SECRET_KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";

/// Kills a spawned child when it goes out of scope so a failed assertion never
/// leaves `nak` or the server running.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Grab a free TCP port by binding to :0 and immediately releasing it. There's
/// an inherent race before the port is reused, but it's fine for a local test.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local_addr")
        .port()
}

/// Poll until something is listening on `port`, or the deadline passes.
fn wait_for_tcp(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Whether `nak` is runnable.
fn nak_available() -> bool {
    Command::new("nak")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A running `relay + bitcoind + contextbtc-server` stack.
///
/// ```text
///   <test> ──MCP/Nostr──▶ nak serve (relay) ──▶ contextbtc-server ──JSON-RPC──▶ bitcoind (regtest)
/// ```
pub struct Stack {
    /// `ws://` URL of the local relay, for clients to connect to.
    pub relay_url: String,
    /// Hex public key the server announced on startup; clients address it.
    pub server_pubkey: String,
    /// The regtest node, for driving the chain directly (mining, etc.).
    pub node: corepc_node::Node,
    // Declared last so the subprocesses outlive anything above that talks to
    // them; fields drop in declaration order.
    _server: ChildGuard,
    _relay: ChildGuard,
}

impl Stack {
    /// Start the whole stack. `extra_bitcoind_args` are appended to the default
    /// regtest arguments (e.g. `-blockfilterindex=1`).
    pub fn start(extra_bitcoind_args: &[&str]) -> anyhow::Result<Self> {
        // --- Preconditions: both external tools must be present, or fail loudly. --
        let bitcoind_exe = corepc_node::exe_path()
            .expect("bitcoind not found: set BITCOIND_EXE or add bitcoind to PATH");
        assert!(
            nak_available(),
            "`nak` not found on PATH (needed to run the local relay via `nak serve`)"
        );

        // --- 1. Local Nostr relay (nak serve) -------------------------------------
        let relay_port = free_port();
        let relay_url = format!("ws://localhost:{relay_port}");
        let nak = Command::new("nak")
            .args(["serve", "--quiet", "--port", &relay_port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let relay_guard = ChildGuard(nak);
        assert!(
            wait_for_tcp(relay_port, Duration::from_secs(10)),
            "relay did not start listening on {relay_port}"
        );

        // --- 2. Regtest bitcoind --------------------------------------------------
        // No wallet: the proxied tools are read-only chain/mempool queries, and
        // creating the default wallet fails on recent Bitcoin Core versions.
        let mut conf = corepc_node::Conf::default();
        conf.wallet = None;
        conf.args.extend_from_slice(extra_bitcoind_args);
        let node = corepc_node::Node::with_conf(&bitcoind_exe, &conf)?;
        let rpc_url = node.rpc_url();
        let cookie = node
            .params
            .get_cookie_values()?
            .expect("regtest node should expose cookie credentials");

        // --- 3. Start the server against bitcoind + the relay ---------------------
        let server_bin = escargot::CargoBuild::new()
            .package("contextbtc-server")
            .bin("contextbtc-server")
            .run()?;
        let mut server = server_bin
            .command()
            .env("SERVER_NOSTR_SECRET_KEY", SERVER_SECRET_KEY)
            .env("NOSTR_RELAY_URLS", &relay_url)
            .env("BITCOIN_RPC_URL", &rpc_url)
            .env("BITCOIN_RPC_USER", &cookie.user)
            .env("BITCOIN_RPC_PASSWORD", &cookie.password)
            .env("RUST_LOG", "warn")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        // Stream the server's stdout on a thread so we can watch for the pubkey it
        // prints at startup.
        let stdout = server.stdout.take().expect("server stdout piped");
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let server_guard = ChildGuard(server);

        // We only wait for the pubkey line here. The server's own "Server ready"
        // log comes from `serve()`, which completes the MCP initialize handshake —
        // and that only happens once a client connects. Blocking on it here would
        // deadlock against the client the test starts next.
        let mut server_pubkey: Option<String> = None;
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && server_pubkey.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(line) => {
                    if let Some(pk) = line.strip_prefix("Public key: ") {
                        server_pubkey = Some(pk.trim().to_string());
                    }
                }
                Err(_) => break,
            }
        }
        let server_pubkey = server_pubkey.expect("server should print its public key");
        println!("Server pub key: {server_pubkey}");

        // Give the server a moment to finish subscribing on the relay before the
        // first client request goes out.
        std::thread::sleep(Duration::from_secs(1));

        Ok(Self {
            relay_url,
            server_pubkey,
            node,
            _server: server_guard,
            _relay: relay_guard,
        })
    }
}
