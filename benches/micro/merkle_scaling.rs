use budlum_core::core::account::{AccountState, Account};
use budlum_core::core::address::Address;
use std::time::Instant;

fn run_bench(account_count: usize, updates: usize, blocks: usize) {
    let mut state = AccountState::new();
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
    state.calculate_state_root();

    let start = Instant::now();
    for b in 0..blocks {
        for i in 0..updates {
            let mut addr_bytes = [0u8; 32];
            let idx = (b * updates + i) % account_count;
            addr_bytes[24..32].copy_from_slice(&(idx as u64).to_be_bytes());
            let addr = Address::from(addr_bytes);
            if let Some(acc) = state.accounts.get_mut(&addr) {
                acc.balance += 1;
                state.mark_dirty(&addr);
            }
        }
        let _ = state.calculate_state_root();
    }
    let duration = start.elapsed();
    let total_updates = blocks * updates;
    println!("Updates per Block: {:<5} | Time per Block: {:<10?} | Total Throughput: {:.2} updates/s", 
             updates, duration / blocks as u32, total_updates as f64 / duration.as_secs_f64());
}

fn main() {
    println!("\n📊 MICRO-BENCHMARK: Merkle Scaling Analysis (O(K log N))");
    println!("------------------------------------------------------------");

    let account_count = 100_000;
    let blocks = 1000;
    
    run_bench(account_count, 10, blocks);
    run_bench(account_count, 100, blocks);
    run_bench(account_count, 1000, blocks);
    run_bench(account_count, 5000, blocks);

    println!("------------------------------------------------------------\n");
}
