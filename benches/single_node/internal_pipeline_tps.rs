use budlum_core::chain::blockchain::Blockchain;
use budlum_core::chain::chain_actor::ChainActor;
use budlum_core::consensus::pos::{PoSEngine, PoSConfig};
use budlum_core::core::transaction::Transaction;
use budlum_core::core::address::Address;
use budlum_core::crypto::primitives::{KeyPair, ValidatorKeys};
use std::sync::Arc;
use std::time::Instant;
use futures::stream::{StreamExt, FuturesUnordered};
use std::pin::Pin;
use std::future::Future;

#[tokio::main]
async fn main() {
    println!("\n🚀 BUDLUM SINGLE-NODE INTERNAL PIPELINE BENCHMARK 🚀");
    println!("--------------------------------------------------");

    // Config
    let tx_count = 10_000;
    let sender_count = 10_000;
    let concurrency = 64;

    // 1. Setup
    let pos_config = PoSConfig::default();
    let validator_keys = ValidatorKeys::generate().unwrap();
    let consensus = Arc::new(PoSEngine::new(pos_config, Some(validator_keys.clone())));
    
    let blockchain = Blockchain::new(consensus, None, 1337, None);
    let (chain_actor, chain) = ChainActor::new(blockchain);
    
    tokio::spawn(async move {
        chain_actor.run().await;
    });

    // 2. Pre-generate Transactions & Fund Accounts
    println!("Generating {} unique senders and funding them...", sender_count);
    let mut keypairs = Vec::new();
    for _ in 0..sender_count {
        let kp = KeyPair::generate().unwrap();
        chain.add_balance(&Address::from(kp.public_key_bytes()), 1_000_000).await;
        keypairs.push(kp);
    }

    println!("Generating {} transactions...", tx_count);
    let start_gen = Instant::now();

    let txs: Vec<Transaction> = (0..tx_count).map(|i| {
        let sender_idx = i % sender_count;
        let mut tx = Transaction::new_with_fee(
            Address::from(keypairs[sender_idx].public_key_bytes()),
            Address::zero(),
            1,
            1,
            (i / sender_count) as u64,
            vec![],
        );
        tx.sign(&keypairs[sender_idx]);
        tx
    }).collect();
    let gen_duration = start_gen.elapsed();
    println!("Generation time: {:?}", gen_duration);

    // 3. Measure INGEST TPS (Mempool Internal Throughput)
    println!("Starting Ingestion (Internal Channel)...");
    let start_ingest = Instant::now();
    let mut futures: FuturesUnordered<Pin<Box<dyn Future<Output = Result<(), String>> + Send>>> = FuturesUnordered::new();
    let mut tx_iter = txs.into_iter();
    
    for _ in 0..concurrency {
        if let Some(tx) = tx_iter.next() {
            let c = chain.clone();
            futures.push(Box::pin(async move { c.add_transaction(tx).await }));
        }
    }
    
    let mut ingested = 0;
    let mut ingest_failed = 0;
    while let Some(res) = futures.next().await {
        match res {
            Ok(_) => ingested += 1,
            Err(_) => ingest_failed += 1,
        }
        
        if let Some(tx) = tx_iter.next() {
            let c = chain.clone();
            futures.push(Box::pin(async move { c.add_transaction(tx).await }));
        }
    }
    let ingest_duration = start_ingest.elapsed();
    let ingest_tps = ingested as f64 / ingest_duration.as_secs_f64();

    // 4. Measure EXECUTION/COMMIT TPS
    println!("Starting Block Production (Execution + State Root)...");
    let start_exec = Instant::now();
    let mut processed = 0;
    let mut blocks = 0;
    let producer = Address::from(validator_keys.sig_key.public_key_bytes());

    while processed < ingested {
        if let Some(block) = chain.produce_block(producer).await {
            processed += block.transactions.len();
            blocks += 1;
            if blocks % 2 == 0 { print!("."); use std::io::Write; std::io::stdout().flush().unwrap(); }
        } else {
            break;
        }
    }
    println!("\nExecution completed.");
    let exec_duration = start_exec.elapsed();
    let exec_tps = processed as f64 / exec_duration.as_secs_f64();

    // 5. Results
    println!("\n--------------------------------------------------");
    println!("BENCHMARK RESULTS (Internal Pipeline):");
    println!("Total Transactions: {} (Failed Ingest: {})", processed, ingest_failed);
    println!("Total Blocks:       {}", blocks);
    println!("--------------------------------------------------");
    println!("TX Generation:      {:?}", gen_duration);
    println!("INGEST TPS (Int):   {:.2} tx/s", ingest_tps);
    println!("EXECUTION TPS:      {:.2} tx/s", exec_tps);
    if blocks > 0 {
        println!("Avg Block Build:   {:?}", exec_duration / blocks as u32);
    } else {
        println!("Avg Block Build:   N/A");
    }
    println!("Note: This bypasses Network, RPC and Serialization.");
    println!("--------------------------------------------------\n");
}
