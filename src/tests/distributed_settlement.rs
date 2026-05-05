#[cfg(test)]
mod distributed_settlement_tests {
    use crate::chain::blockchain::Blockchain;
    use crate::chain::chain_actor::{ChainActor, ChainHandle};
    use crate::consensus::pow::PoWEngine;
    use crate::core::address::Address;
    use crate::core::block::Block;
    use crate::domain::plugin::default_domain;
    use crate::domain::{ConsensusKind, DomainCommitment, DomainStatus};
    use crate::network::node::{Node, NodeClient};
    use crate::storage::db::Storage;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::sleep;

    struct NodeHarness {
        node_client: NodeClient,
        chain_handle: ChainHandle,
        _storage: Arc<Storage>,
        _temp_dir: TempDir,
        node_task: tokio::task::JoinHandle<()>,
        actor_task: tokio::task::JoinHandle<()>,
        port: u16,
    }

    impl NodeHarness {
        async fn new(port: u16, bootstrap: Vec<String>, data_path: Option<PathBuf>) -> Self {
            let temp_dir = TempDir::new().unwrap();
            let path = data_path.unwrap_or_else(|| temp_dir.path().to_path_buf());
            let storage = Arc::new(Storage::new(path.to_str().unwrap()).unwrap());

            let consensus = Arc::new(PoWEngine::new(0));
            let mut blockchain = Blockchain::new(consensus, Some((*storage).clone()), 1337, None);

            let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
            let pos = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
            let _ = blockchain.register_consensus_domain(pow);
            let _ = blockchain.register_consensus_domain(pos);

            let (chain_actor, chain_handle) = ChainActor::new(blockchain);
            let actor_task = tokio::spawn(async move {
                chain_actor.run().await;
            });

            let mut node = Node::new(chain_handle.clone()).unwrap();
            node.bootstrap_peers = bootstrap;
            let _ = node.listen(port);
            let node_client = node.get_client();
            let mut node_instance = node;
            let node_task = tokio::spawn(async move {
                node_instance.run().await;
            });

            sleep(Duration::from_millis(1000)).await;

            Self {
                node_client,
                chain_handle,
                _storage: storage,
                _temp_dir: temp_dir,
                node_task,
                actor_task,
                port,
            }
        }

        fn multiaddr(&self) -> String {
            format!("/ip4/127.0.0.1/tcp/{}", self.port)
        }

        async fn stop(self) {
            self.node_task.abort();
            self.actor_task.abort();
            sleep(Duration::from_millis(200)).await;
        }
    }

    #[tokio::test]
    async fn test_distributed_gossip_convergence() {
        let n1 = NodeHarness::new(5101, vec![], None).await;
        let n2 = NodeHarness::new(5102, vec![n1.multiaddr()], None).await;
        let n3 = NodeHarness::new(5103, vec![n1.multiaddr()], None).await;
        let n4 = NodeHarness::new(5104, vec![n2.multiaddr()], None).await;
        let n5 = NodeHarness::new(5105, vec![n3.multiaddr()], None).await;

        sleep(Duration::from_secs(5)).await;

        let alice = Address::from([1u8; 32]);
        let mut commitments = Vec::new();
        for i in 1..=20 {
            let mut b = Block::new(i, "hash".into(), vec![]);
            b.hash = format!("hash_{}", i).repeat(16)[0..64].to_string();
            let mut com = DomainCommitment::from_block(
                &default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
                &b,
                [0u8; 32],
                [0u8; 32],
                i as u64,
            )
            .unwrap();
            com.state_updates.insert(alice, i as u64);
            commitments.push(com);
        }

        use rand::Rng;
        let mut rng = rand::thread_rng();
        let clients = vec![&n1, &n2, &n3, &n4, &n5];

        for com in commitments {
            let idx = rng.gen_range(0..clients.len());
            let sender = clients[idx];
            sender.node_client.broadcast_domain_commitment_sync(com);
            sleep(Duration::from_millis(100)).await;
        }

        sleep(Duration::from_secs(10)).await;

        let h1 = n1.chain_handle.build_global_header(None).await.unwrap();
        let h2 = n2.chain_handle.build_global_header(None).await.unwrap();
        let h3 = n3.chain_handle.build_global_header(None).await.unwrap();
        let h4 = n4.chain_handle.build_global_header(None).await.unwrap();
        let h5 = n5.chain_handle.build_global_header(None).await.unwrap();

        assert_eq!(h1.domain_commitment_root, h2.domain_commitment_root);
        assert_eq!(h2.domain_commitment_root, h3.domain_commitment_root);
        assert_eq!(h3.domain_commitment_root, h4.domain_commitment_root);
        assert_eq!(h4.domain_commitment_root, h5.domain_commitment_root);
        assert_ne!(
            h1.domain_commitment_root, [0u8; 32],
            "Root should not be empty"
        );

        n1.stop().await;
        n2.stop().await;
        n3.stop().await;
        n4.stop().await;
        n5.stop().await;
    }

    #[tokio::test]
    async fn test_restart_pending_buffer_persistence() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().to_path_buf();

        let n1 = NodeHarness::new(6101, vec![], Some(path.clone())).await;
        let mut b2 = Block::new(2, "h2".into(), vec![]);
        b2.hash = "hash_2".repeat(8);
        let com2 = DomainCommitment::from_block(
            &default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
            &b2,
            [0u8; 32],
            [0u8; 32],
            1,
        )
        .unwrap();

        n1.chain_handle
            .submit_domain_commitment(com2)
            .await
            .unwrap();
        assert_eq!(
            n1.chain_handle.get_domain_height(1).await.unwrap(),
            0,
            "Height 2 should be pending"
        );

        n1.stop().await;

        let n2 = NodeHarness::new(6101, vec![], Some(path.clone())).await;
        let mut b1 = Block::new(1, "h1".into(), vec![]);
        b1.hash = "hash_1".repeat(8);
        let com1 = DomainCommitment::from_block(
            &default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
            &b1,
            [0u8; 32],
            [0u8; 32],
            1,
        )
        .unwrap();

        n2.chain_handle
            .submit_domain_commitment(com1)
            .await
            .unwrap();

        sleep(Duration::from_millis(500)).await;
        assert_eq!(
            n2.chain_handle.get_domain_height(1).await.unwrap(),
            2,
            "Height 2 should be applied after restart and H1"
        );

        n2.stop().await;
    }

    #[tokio::test]
    async fn test_frozen_domain_persistence() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().to_path_buf();

        let n1 = NodeHarness::new(7101, vec![], Some(path.clone())).await;
        let b1 = Block::new(1, "h1".into(), vec![]);
        let b1_alt = Block::new(1, "h1_alt".into(), vec![]);
        let com1 = DomainCommitment::from_block(
            &default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
            &b1,
            [0u8; 32],
            [0u8; 32],
            1,
        )
        .unwrap();
        let mut com1_alt = DomainCommitment::from_block(
            &default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
            &b1_alt,
            [0u8; 32],
            [0u8; 32],
            2,
        )
        .unwrap();
        com1_alt.domain_height = 1;

        n1.chain_handle
            .submit_domain_commitment(com1)
            .await
            .unwrap();
        let res = n1.chain_handle.submit_domain_commitment(com1_alt).await;
        assert!(res.is_err(), "Equivocation must be rejected");

        n1.stop().await;

        let n2 = NodeHarness::new(7101, vec![], Some(path.clone())).await;
        let b2 = Block::new(2, "h2".into(), vec![]);
        let com2 = DomainCommitment::from_block(
            &default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
            &b2,
            [0u8; 32],
            [0u8; 32],
            3,
        )
        .unwrap();

        let res2 = n2.chain_handle.submit_domain_commitment(com2).await;
        assert!(res2.is_err());
        assert!(res2.unwrap_err().contains("frozen"));

        n2.stop().await;
    }

    #[tokio::test]
    async fn test_conflicting_commitment_audit_persistence() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().to_path_buf();
        let alice = Address::from([1u8; 32]);

        let n1 = NodeHarness::new(8101, vec![], Some(path.clone())).await;
        let mut b1 = Block::new(1, "h1".into(), vec![]);
        b1.hash = "h1".repeat(32);
        let mut com_pow = DomainCommitment::from_block(
            &default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
            &b1,
            [0u8; 32],
            [0u8; 32],
            1,
        )
        .unwrap();
        com_pow.state_updates.insert(alice, 1);

        n1.chain_handle
            .submit_domain_commitment(com_pow.clone())
            .await
            .unwrap();

        let mut b2 = Block::new(1, "h2".into(), vec![]);
        b2.hash = "h2".repeat(32);
        let mut com_pos = DomainCommitment::from_block(
            &default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0),
            &b2,
            [0u8; 32],
            [0u8; 32],
            1,
        )
        .unwrap();
        com_pos.state_updates.insert(alice, 1);

        n1.chain_handle
            .submit_domain_commitment(com_pos.clone())
            .await
            .unwrap();

        n1.stop().await;

        let n2 = NodeHarness::new(8101, vec![], Some(path.clone())).await;
        let header = n2.chain_handle.build_global_header(None).await.unwrap();
        assert_ne!(header.domain_commitment_root, [0u8; 32]);

        n2.stop().await;
    }

    #[tokio::test]
    async fn test_adversarial_finality_proofs() {
        let n = NodeHarness::new(9101, vec![], None).await;
        let alice = Address::from([1u8; 32]);
        let mut b = Block::new(1, "h".into(), vec![]);
        b.hash = "h".repeat(32);
        let pow_domain = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);

        use crate::domain::finality_adapter::{hash_finality_proof, FinalityProof};

        let mut com =
            DomainCommitment::from_block(&pow_domain, &b, [0u8; 32], [0u8; 32], 1).unwrap();
        com.state_updates.insert(alice, 1);
        let proof = FinalityProof::PoW {
            confirmations: 100,
            total_work_hint: 5000,
        };
        com.finality_proof_hash = [0xFFu8; 32];

        let res = n
            .chain_handle
            .submit_verified_domain_commitment(crate::domain::VerifiedDomainCommitment {
                commitment: com,
                proof,
            })
            .await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("mismatch"));

        let mut com2 =
            DomainCommitment::from_block(&pow_domain, &b, [0u8; 32], [0u8; 32], 2).unwrap();
        let proof2 = FinalityProof::PoW {
            confirmations: 1,
            total_work_hint: 5000,
        };
        com2.finality_proof_hash = hash_finality_proof(&proof2);

        let res2 = n
            .chain_handle
            .submit_verified_domain_commitment(crate::domain::VerifiedDomainCommitment {
                commitment: com2,
                proof: proof2,
            })
            .await;
        assert!(res2.is_err());
        assert!(res2.unwrap_err().contains("not finalized"));
        assert_eq!(
            n.chain_handle.get_domain_height(1).await.unwrap(),
            0,
            "Should NOT be applied"
        );

        n.stop().await;
    }
}
