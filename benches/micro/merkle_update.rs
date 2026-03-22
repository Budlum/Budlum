use budlum_core::core::account::{AccountState, Account};
use budlum_core::core::address::Address;
use std::time::Instant;

fn main() {
    println!("\n📊 MICRO-BENCHMARK: Incremental Merkle Tree Update (O(log N))");
    println!("------------------------------------------------------------");

    let account_count = 100_000;
    let mut state = AccountState::new();
    
    println!("Initializing state with {} accounts...", account_count);
    for i in 0..account_count {
        let mut addr_bytes = [0u8; 32];
        addr_bytes[24..32].copy_from_slice(&(i as u64).to_be_bytes());
        let addr = Address::from(addr_bytes);
        state.accounts.insert(addr, Account {
            public_key: addr,
            balance: 1000,
            nonce: 0,
        });
    }

    println!("Performing initial full tree calculation...");
    let start_init = Instant::now();
    let root = state.calculate_state_root();
    println!("Initial Root: {} (Time: {:?})", root, start_init.elapsed());

    let updates_per_block = 100;
    let blocks = 1000;
    println!("Starting benchmark: {} blocks with {} updates each...", blocks, updates_per_block);
    
    let start_bench = Instant::now();
    for b in 0..blocks {
        for i in 0..updates_per_block {
            let mut addr_bytes = [0u8; 32];
            let idx = (b * updates_per_block + i) % account_count;
            addr_bytes[24..32].copy_from_slice(&(idx as u64).to_be_bytes());
            let addr = Address::from(addr_bytes);
            if let Some(acc) = state.accounts.get_mut(&addr) {
                acc.balance += 1;
                state.mark_dirty(&addr);
            }
        }
        let _ = state.calculate_state_root();
    }
    
    let duration = start_bench.elapsed();
    let total_updates = blocks * updates_per_block;
    
    println!("Total Time:       {:?}", duration);
    println!("Avg per Block:    {:?}", duration / blocks as u32);
    println!("Avg per Update:   {:?}", duration / total_updates as u32);
    println!("Throughput:       {:.2} updates/s", total_updates as f64 / duration.as_secs_f64());
    println!("------------------------------------------------------------\n");
}
