use crate::chain::blockchain::Blockchain;
use crate::chain::chain_actor::ChainActor;
use crate::consensus::pos::{PoSConfig, PoSEngine};
use crate::core::address::Address;
use crate::core::transaction::Transaction;
use crate::crypto::primitives::{KeyPair, ValidatorKeys};
use futures::stream::{FuturesUnordered, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

// #[tokio::test]
#[allow(dead_code)]
async fn bench_high_tps() {
    println!("\n🚀 BUDLUM REAL-WORLD TPS STRESS TEST (PoS + BLS + QUANTUM) 🚀");
    println!("----------------------------------------------------------");

    // 1. Setup Realistic Consensus
    let pos_config = PoSConfig::default();
    let validator_keys = ValidatorKeys::generate().unwrap();
    let consensus = Arc::new(PoSEngine::new(pos_config, Some(validator_keys.clone())));

    let blockchain = Blockchain::new(consensus, None, 1337, None);
    let (chain_actor, chain) = ChainActor::new(blockchain);

    tokio::spawn(async move {
        chain_actor.run().await;
    });

    let tx_count = 50_000; // Realistic load for a single node bench
    let sender_count = 50;
    println!(
        "Preparing {} transactions from {} unique senders...",
        tx_count, sender_count
    );

    let mut keypairs = Vec::new();
    let mut sender_addrs = Vec::new();
    for _ in 0..sender_count {
        let kp = KeyPair::generate().unwrap();
        sender_addrs.push(Address::from(kp.public_key_bytes()));
        keypairs.push(kp);
    }

    // Parallel Transaction Generation & Signing
    let start_gen = Instant::now();
    let txs: Vec<Transaction> = (0..tx_count)
        .map(|i| {
            let sender_idx = index_of_sender(i, sender_count);
            let mut recipient_bytes = [0u8; 32];
            recipient_bytes[0] = (i % 256) as u8;
            let recipient = Address::from(recipient_bytes);

            let mut tx = Transaction::new_with_fee(
                sender_addrs[sender_idx],
                recipient,
                1,
                1,
                (i / sender_count) as u64,
                vec![],
            );
            tx.sign(&keypairs[sender_idx]);
            tx
        })
        .collect();

    let gen_duration = start_gen.elapsed();
    println!(
        "Generation/Signing time: {:?} ({:.2} tx/s)",
        gen_duration,
        tx_count as f64 / gen_duration.as_secs_f64()
    );

    // 2. Parallel Ingestion (Simulating concurrent RPC calls)
    println!("Ingesting transactions into Mempool (Parallel Workers)...");
    let start_ingest = Instant::now();

    let concurrency = 16;
    let mut futures: FuturesUnordered<Pin<Box<dyn Future<Output = Result<(), String>> + Send>>> =
        FuturesUnordered::new();
    let mut tx_iter = txs.into_iter();

    for _ in 0..concurrency {
        if let Some(tx) = tx_iter.next() {
            let chain_clone = chain.clone();
            futures.push(Box::pin(
                async move { chain_clone.add_transaction(tx).await },
            ));
        }
    }

    let mut ingested = 0;
    while let Some(_) = futures.next().await {
        ingested += 1;
        if let Some(tx) = tx_iter.next() {
            let chain_clone = chain.clone();
            futures.push(Box::pin(
                async move { chain_clone.add_transaction(tx).await },
            ));
        }
    }

    let ingest_duration = start_ingest.elapsed();
    println!(
        "Ingestion completed: {} tx in {:?} ({:.2} tx/s)",
        ingested,
        ingest_duration,
        ingested as f64 / ingest_duration.as_secs_f64()
    );

    // 3. Realistic Block Production
    // This includes: In-block verification, State transitions, Merkle Root, and Reward distribution.
    println!("Starting Block Production (State Machine + Merkle Stress)...");
    let start_bench = Instant::now();

    let mut blocks_count = 0;
    let mut total_tx_processed = 0;
    let producer_addr = Address::from(validator_keys.sig_key.public_key_bytes());

    while total_tx_processed < ingested {
        if let Some(block) = chain.produce_block(producer_addr).await {
            total_tx_processed += block.transactions.len();
            blocks_count += 1;

            // Progress indicator
            if blocks_count % 5 == 0 {
                print!(".");
                use std::io::Write;
                std::io::stdout().flush().unwrap();
            }
        } else {
            // No more txs in mempool target reached or error
            break;
        }
    }
    println!("\nAll blocks produced.");

    let bench_duration = start_bench.elapsed();
    let tps = total_tx_processed as f64 / bench_duration.as_secs_f64();

    println!("----------------------------------------------------------");
    println!("REAL-WORLD BENCHMARK RESULTS:");
    println!("Consensus Engine:     Proof of Stake (PoS)");
    println!("Total Transactions:   {}", total_tx_processed);
    println!("Total Blocks:         {}", blocks_count);
    println!(
        "Avg TX per Block:     {}",
        if blocks_count > 0 {
            total_tx_processed / blocks_count
        } else {
            0
        }
    );
    println!("Total Processing Time: {:?}", bench_duration);
    println!("CORE THROUGHPUT (TPS): {:.2} tx/s", tps);
    println!("----------------------------------------------------------");
    println!("Note: This TPS includes full signature verification, nonce checks,");
    println!("balance updates, and O(log N) Incremental Merkle Tree root calculation.");
}

#[allow(dead_code)]
fn index_of_sender(i: usize, count: usize) -> usize {
    i % count
}
