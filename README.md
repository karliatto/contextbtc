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

## Running server

```bash
NOSTR_SECRET_KEY=<secret-key-hex> \
BITCOIN_RPC_URL=http://127.0.0.1:18443 \
BITCOIN_RPC_USER=myuser \
BITCOIN_RPC_PASSWORD=mypass \
cargo run -- server
```

`NOSTR_SECRET_KEY` accepts a 64-char hex key or an `nsec...` key. If it is unset, the server generates a temporary key on each start (fine for testing, not for production). Do not commit your secret key.

## Running client

```bash
cargo run -- client <server-pub-key-hex>
```

## Architecture

This project bridges two distinct protocol layers:

- **Client ⟷ ContexVM MCP server:** MCP over Nostr.
- **ContexVM MCP server ⟷ bitcoind:** JSON-RPC over HTTP.
