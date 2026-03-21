use crate::chain::blockchain::Blockchain;
use crate::chain::chain_actor::ChainActor;
use crate::consensus::pow::PoWEngine;
use crate::core::transaction::{Transaction, TransactionType};
use crate::crypto::primitives::KeyPair;
use std::sync::Arc;
use std::time::Instant;

//#[tokio::test]
async fn bench_high_tps() {
    println!("\n🚀 BUDLUM HIGH-PERFORMANCE BENCHMARK 🚀");
    println!("---------------------------------------");

    let consensus = Arc::new(PoWEngine::new(0));
    let blockchain = Blockchain::new(consensus, None, 1337, None);
    let (chain_actor, chain) = ChainActor::new(blockchain);
    
    tokio::spawn(async move {
        chain_actor.run().await;
    });

    let tx_count = 50_000;
    println!("Generating {} transactions...", tx_count);
    let start_gen = Instant::now();
    let keypair = KeyPair::generate().unwrap();
    let recipient = "0".repeat(64);
    
    let mut txs = Vec::with_capacity(tx_count);
    for i in 0..tx_count {
        let mut tx = Transaction::new_with_fee(
            keypair.public_key_hex(),
            recipient.clone(),
            1,
            1,
            i as u64,
            vec![],
        );
        tx.sign(&keypair);
        txs.push(tx);
    }
    let gen_duration = start_gen.elapsed();
    println!("Generation time: {:?} ({:.2} tx/s)", gen_duration, tx_count as f64 / gen_duration.as_secs_f64());

    println!("Starting block production bench...");
    let start_bench = Instant::now();
    
    for tx in txs {
        let _ = chain.add_transaction(tx).await;
    }
    
    let mut blocks_count = 0;
    let mut total_tx_processed = 0;
    
    while total_tx_processed < tx_count {
        if let Some(block) = chain.produce_block(keypair.public_key_hex()).await {
            total_tx_processed += block.transactions.len();
            blocks_count += 1;
        } else {
            break;
        }
    }

    let bench_duration = start_bench.elapsed();
    let tps = total_tx_processed as f64 / bench_duration.as_secs_f64();
    
    println!("---------------------------------------");
    println!("BENCHMARK RESULTS:");
    println!("Total Transactions: {}", total_tx_processed);
    println!("Total Blocks:       {}", blocks_count);
    println!("Total Time:         {:?}", bench_duration);
    println!("THROUGHPUT (TPS):   {:.2} tx/s", tps);
    println!("---------------------------------------");
}
