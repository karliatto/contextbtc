//! End-to-end compact block filter (BIP157/158) sync over MCP-over-Nostr.
//!
//! ```text
//!   bdk FilterIter ──MCP/Nostr──▶ nak serve (relay) ──▶ contextbtc-server ──JSON-RPC──▶ bitcoind (regtest)
//! ```
//!
//! This shows BDK's chain and tx-graph structures being updated by compact
//! filter syncing where the "Bitcoin Core RPC client" is in fact this repo's
//! `bitcoincore-rpc` stand-in, which turns every call into an MCP `tools/call`
//! over Nostr. `bdk_bitcoind_rpc` itself is the unmodified crates.io release —
//! it is swapped onto the Nostr transport purely by the workspace's
//! `[patch.crates-io]`, and knows nothing about any of this.
//!
//! The test drives a regtest chain to a known shape: filler blocks up to
//! [`START_HEIGHT`], one block whose coinbase pays a descriptor address, then a
//! few more filler blocks. It then syncs from `START_HEIGHT` and asserts the
//! funded block was matched by its filter and its output landed in the graph.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use bdk_bitcoind_rpc::bip158::{Event, FilterIter};
use bdk_chain::bitcoin::{
    Address, Amount, BlockHash, CompressedPublicKey, Network, PrivateKey, constants::genesis_block,
    secp256k1::Secp256k1,
};
use bdk_chain::indexer::keychain_txout::KeychainTxOutIndex;
use bdk_chain::local_chain::LocalChain;
use bdk_chain::miniscript::Descriptor;
use bdk_chain::{BlockId, ConfirmationBlockTime, IndexedTxGraph, SpkIterator};

mod harness;

use harness::Stack;

const EXTERNAL: &str = "tr([83737d5e/86'/1'/0']tpubDDR5GgtoxS8fJyjjvdahN4VzV5DV6jtbcyvVXhEKq2XtpxjxBXmxH3r8QrNbQqHg4bJM1EGkxi7Pjfkgnui9jQWqS7kxHvX6rhUeriLDKxz/0/*)";
const INTERNAL: &str = "tr([83737d5e/86'/1'/0']tpubDDR5GgtoxS8fJyjjvdahN4VzV5DV6jtbcyvVXhEKq2XtpxjxBXmxH3r8QrNbQqHg4bJM1EGkxi7Pjfkgnui9jQWqS7kxHvX6rhUeriLDKxz/1/*)";
const SPK_COUNT: u32 = 5;
const NETWORK: Network = Network::Regtest;

/// The height of the block at which we start the sync. Unlike a birthday height
/// on a real chain, here we mine to it — so the hash is read back from the node
/// rather than hardcoded.
const START_HEIGHT: u32 = 20;
/// Derivation index (on the external keychain) of the address the funded
/// block's coinbase pays to. Must be `< SPK_COUNT` to be in the watched set.
const FUNDED_INDEX: u32 = 0;
/// Filler blocks mined after the funded one, so the sync has to walk past it.
const BLOCKS_AFTER_FUNDING: usize = 5;

#[test]
fn filter_iter_syncs_descriptor_over_nostr() -> anyhow::Result<()> {
    // Install a logger so `log` macros in `bitcoincore_rpc` (which logs every
    // MCP tools/call) actually emit. Control verbosity with `RUST_LOG`; the
    // default keeps the transport chatter out of the way. `cargo test` only
    // shows this output for failing tests.
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("bitcoincore_rpc=debug"),
    )
    .is_test(true)
    .try_init();

    // `-blockfilterindex=1` is what makes `getblockfilter` available at all.
    let stack = Stack::start(&["-blockfilterindex=1"])?;

    // --- Setup receiving chain and graph structures ---------------------------

    let secp = Secp256k1::new();
    let (descriptor, _) = Descriptor::parse_descriptor(&secp, EXTERNAL)?;
    let (change_descriptor, _) = Descriptor::parse_descriptor(&secp, INTERNAL)?;
    let (mut chain, _) = LocalChain::from_genesis_hash(genesis_block(NETWORK).block_hash());

    let mut graph = IndexedTxGraph::<ConfirmationBlockTime, KeychainTxOutIndex<&str>>::new({
        let mut index = KeychainTxOutIndex::default();
        index.insert_descriptor("external", descriptor.clone())?;
        index.insert_descriptor("internal", change_descriptor.clone())?;
        index
    });

    // --- Drive the regtest chain into a known shape ---------------------------

    // Filler blocks pay a throwaway key unrelated to either descriptor, so only
    // the funded block below should match the watched SPKs.
    let filler_address = throwaway_address(&secp)?;
    let funded_address = descriptor
        .at_derivation_index(FUNDED_INDEX)?
        .address(NETWORK)?;

    let client = &stack.node.client;
    client.generate_to_address(START_HEIGHT as usize, &filler_address)?;
    // Blocks 1..=START_HEIGHT are filler; this one funds us.
    client.generate_to_address(1, &funded_address)?;
    let funded_height = START_HEIGHT + 1;
    client.generate_to_address(BLOCKS_AFTER_FUNDING, &filler_address)?;
    let tip_height = funded_height + BLOCKS_AFTER_FUNDING as u32;

    // bitcoind builds the block filter index asynchronously, so a filter for a
    // freshly mined block is not necessarily available the instant it connects.
    let tip_hash = block_hash_at(&stack, tip_height)?;
    wait_for_block_filter(&stack, tip_hash, Duration::from_secs(30))?;

    // Start the sync from START_HEIGHT rather than genesis, the way a wallet
    // would start from its birthday height.
    let start_hash = block_hash_at(&stack, START_HEIGHT)?;
    let _ = chain.insert_block(BlockId {
        height: START_HEIGHT,
        hash: start_hash,
    })?;

    // --- Configure the RPC client (MCP over Nostr) ----------------------------

    let rpc_client =
        bitcoincore_rpc::Client::new(vec![stack.relay_url.clone()], stack.server_pubkey.clone())?;

    // Diagnostic: dump the server's advertised tools and their input schemas.
    rpc_client.dump_tool_schemas()?;

    // --- Initialize `FilterIter` ----------------------------------------------

    let mut spks = vec![];
    // graph.index.keychains() yields one entry per keychain: external and internal.
    for (_, desc) in graph.index.keychains() {
        println!("desc: {desc:?}");
        // The range of script pubkeys (scriptPubKeys) to sync.
        spks.extend(SpkIterator::new_with_range(desc, 0..SPK_COUNT).map(|(_, s)| s));
    }
    println!("spks: {spks:?}");

    let iter = FilterIter::new(&rpc_client, chain.tip(), spks);

    let start = Instant::now();
    let mut matched_heights = BTreeSet::new();

    for res in iter {
        let Event { cp, block } = res?;
        let height = cp.height();
        let _ = chain.apply_update(cp)?;
        if let Some(block) = block {
            // apply_block_relevant returns a ChangeSet, useful to persist, but
            // here we are not persisting it.
            let _ = graph.apply_block_relevant(&block, height);
            matched_heights.insert(height);
            println!("Matched block {height}");
        }
    }

    println!("graph: {:?}", graph.index.outpoints());
    println!("\ntook: {}s", start.elapsed().as_secs());
    println!("Local tip: {}", chain.tip().height());

    // --- Assertions -----------------------------------------------------------

    assert_eq!(
        chain.tip().height(),
        tip_height,
        "sync should have walked the local chain up to the node's tip"
    );
    // A BIP158 filter can produce false positives, so other heights may appear
    // here; only the funded one is guaranteed.
    assert!(
        matched_heights.contains(&funded_height),
        "filter should have matched the block funding the descriptor \
         (height {funded_height}); matched: {matched_heights:?}"
    );

    let unspent: Vec<_> = graph
        .graph()
        .filter_chain_unspents(
            &chain,
            chain.tip().block_id(),
            Default::default(),
            graph.index.outpoints().clone(),
        )
        .collect();
    println!("\nUnspent");
    for (index, utxo) in &unspent {
        // (k, index) | value | outpoint |
        println!("{:?} | {} | {}", index, utxo.txout.value, utxo.outpoint);
    }

    assert_eq!(
        unspent.len(),
        1,
        "exactly one output (the funded coinbase) should be unspent: {unspent:?}"
    );
    let (keychain_index, utxo) = &unspent[0];
    assert_eq!(*keychain_index, ("external", FUNDED_INDEX));
    assert!(utxo.is_on_coinbase);
    // Regtest halves every 150 blocks, so the subsidy at this height is 50 BTC
    // and there are no fees to collect.
    assert_eq!(utxo.txout.value, Amount::from_int_btc(50));
    assert_eq!(
        utxo.chain_position.confirmation_height_upper_bound(),
        Some(funded_height)
    );

    // A canonical tx is one the graph considers in-chain. Everything we synced
    // came out of a block, so nothing should be unconfirmed.
    for canon_tx in graph.graph().list_canonical_txs(
        &chain,
        chain.tip().block_id(),
        bdk_chain::CanonicalizationParams::default(),
    ) {
        assert!(
            canon_tx.chain_position.is_confirmed(),
            "canonical tx should be confirmed {}",
            canon_tx.tx_node.txid
        );
    }

    Ok(())
}

/// A regtest address for a deterministic throwaway key, used to mine blocks
/// that must not match the watched descriptors.
fn throwaway_address(
    secp: &Secp256k1<bdk_chain::bitcoin::secp256k1::All>,
) -> anyhow::Result<Address> {
    let sk = PrivateKey::from_slice(&[0x42; 32], NETWORK)?;
    let pk = CompressedPublicKey::from_private_key(secp, &sk)?;
    Ok(Address::p2wpkh(&pk, NETWORK))
}

/// Read a block hash straight from the node (not through the Nostr transport).
fn block_hash_at(stack: &Stack, height: u32) -> anyhow::Result<BlockHash> {
    Ok(stack
        .node
        .client
        .get_block_hash(height as u64)?
        .block_hash()?)
}

/// Poll bitcoind until it can serve the basic filter for `hash`, since the
/// block filter index is populated asynchronously after a block connects.
fn wait_for_block_filter(stack: &Stack, hash: BlockHash, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    let args = [
        serde_json::Value::from(hash.to_string()),
        serde_json::Value::from("basic"),
    ];
    let mut last_err = None;
    while Instant::now() < deadline {
        match stack
            .node
            .client
            .call::<serde_json::Value>("getblockfilter", &args)
        {
            Ok(_) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("block filter index did not catch up within {timeout:?}: {last_err:?}")
}
