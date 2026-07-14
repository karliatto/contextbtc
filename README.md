# ContextBTC Rust

ContextBTC Rust provides a [Model Context Protocol (MCP)](https://modelcontextprotocol.io) interface to a Bitcoin Core node, using [ContextVM](https://github.com/contextvm) to transport MCP messages over Nostr. Nostr's cryptographic keypairs and signed events provide built-in verification and authorization.

## Generating a Nostr key

The server needs a stable Nostr identity. Generate a secret key with [nak](https://github.com/fiatjaf/nak) (included in the dev shell):

```bash
nak key generate
# -> 7b94e287...bc6148d  (64-char hex secret key)
```

Derive the public key (what clients target) from a secret key with:

```bash
nak key public <secret-key-hex>
```

## Configuration

The server is configured via environment variables. For local development, copy
the provided template and fill in your values:

```bash
cp .env.example .env
# edit .env
```

On startup the server automatically loads a `.env` file if present. Real
environment variables always take precedence over `.env`, and a missing file is
not an error (useful for systemd/Docker where variables are injected directly).
`.env` is gitignored, so your secrets are never committed.

| Variable | Required | Default | Description |
| --- | --- | --- | --- |
| `SERVER_NOSTR_SECRET_KEY` | No | ephemeral | 64-char hex or `nsec...` key. If unset, a temporary key is generated on each start (testing only, not for production). |
| `BITCOIN_RPC_URL` | No | `http://127.0.0.1:8332` | Bitcoin Core JSON-RPC endpoint. |
| `BITCOIN_RPC_USER` | Yes | — | JSON-RPC username. |
| `BITCOIN_RPC_PASSWORD` | Yes | — | JSON-RPC password. |
| `BITCOIN_RPC_TIMEOUT_SECS` | No | `30` | Overall HTTP request timeout for RPC calls, in seconds. |

## Project layout

This is a Cargo workspace with two binary crates:

- `crates/server` — the ContextBTC MCP server (`context-btc-server`).
- `crates/client` — an example client (`context-btc-client`).

## Running server

With a `.env` file in place:

```bash
cargo run -p context-btc-server
```

Alternatively, set variables inline (these override any `.env` values):

```bash
SERVER_NOSTR_SECRET_KEY=<secret-key-hex> \
BITCOIN_RPC_URL=http://127.0.0.1:18443 \
BITCOIN_RPC_USER=myuser \
BITCOIN_RPC_PASSWORD=mypass \
cargo run -p context-btc-server
```

## Running client

## Client .env

```bash
CLIENT_NOSTR_SECRET_KEY=
```

```bash
cargo run -p context-btc-client -- <server-pub-key-hex>
```

## Architecture

This project bridges two distinct protocol layers:

- **Client ⟷ ContexVM MCP server:** MCP over Nostr.
- **ContexVM MCP server ⟷ bitcoind:** JSON-RPC over HTTP.
