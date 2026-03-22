use budlum_core::crypto::primitives::KeyPair;
use std::time::Instant;

fn main() {
    println!("\n📊 MICRO-BENCHMARK: Signature Verification (Ed25519 + Sha3)");
    println!("----------------------------------------------------------");

    let count = 100_000;
    println!("Generating {} keypairs and signatures...", count);
    
    let kp = KeyPair::generate().unwrap();
    let sample_count = 1000;
    println!("Generating {} unique samples to avoid cache bias...", sample_count);
    
    let samples: Vec<(Vec<u8>, [u8; 64])> = (0..sample_count).map(|i| {
        let msg = format!("Budlum stress message {}", i).into_bytes();
        let sig = kp.sign(&msg);
        (msg, sig)
    }).collect();
    
    let pk_bytes = kp.public_key_bytes();

    println!("Starting verification loop ({} total iterations)...", count);
    let start = Instant::now();
    
    for i in 0..count {
        let (msg, sig) = &samples[i % sample_count];
        if i == 0 {
            budlum_core::crypto::primitives::verify_signature(msg, sig, &pk_bytes).expect("Initial signature failure");
        } else {
            let _ = budlum_core::crypto::primitives::verify_signature(msg, sig, &pk_bytes);
        }
    }
    
    let duration = start.elapsed();
    let ops_per_sec = count as f64 / duration.as_secs_f64();
    
    println!("Total Time:       {:?}", duration);
    println!("Avg per Verify:   {:?}", duration / count as u32);
    println!("Throughput:       {:.2} verify/s", ops_per_sec);
    println!("----------------------------------------------------------\n");
}
