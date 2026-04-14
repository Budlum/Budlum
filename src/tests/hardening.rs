#[cfg(test)]
mod hardening_tests {
    use crate::cli::commands::NodeConfig;
    use crate::core::account::AccountState;
    use crate::core::address::Address;
    use crate::core::metrics::Metrics;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_merkle_state_root_determinism() {
        let mut state1 = AccountState::new();
        let alice = Address::from_hex(&"01".repeat(32)).unwrap();
        let bob = Address::from_hex(&"02".repeat(32)).unwrap();

        state1.add_balance(&alice, 100);
        state1.add_balance(&bob, 200);

        let mut state2 = AccountState::new();
        state2.add_balance(&bob, 200);
        state2.add_balance(&alice, 100);

        let root1 = state1.calculate_state_root();
        let root2 = state2.calculate_state_root();

        assert_eq!(
            root1, root2,
            "Merkle root must be deterministic regardless of insertion order"
        );
        assert_ne!(root1, "0".repeat(64), "Root should not be empty");

        state1.add_balance(&alice, 1);
        assert_ne!(
            root1,
            state1.calculate_state_root(),
            "Root must change when balance changes"
        );
    }

    #[test]
    fn test_metrics_encoding_format() {
        let metrics = Metrics::new();
        metrics.chain_height.set(1234);
        metrics.peer_count.set(5);

        let encoded = metrics.encode();
        assert!(
            encoded.contains("budlum_chain_height 1234"),
            "Encoded metrics should contain height"
        );
        assert!(
            encoded.contains("budlum_peer_count 5"),
            "Encoded metrics should contain peer count"
        );
        assert!(
            encoded.contains("# HELP budlum_chain_height"),
            "Encoded metrics should contain HELP metadata"
        );
    }

    #[test]
    fn test_toml_config_merge() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("budlum.toml");
        let mut file = File::create(&config_path).unwrap();
        writeln!(
            file,
            r#"
            db_path = "/tmp/custom_db"
            rpc_port = 9999
            metrics_port = 7070
        "#
        )
        .unwrap();

        let mut config = NodeConfig::default();
        config.config = Some(config_path.to_str().unwrap().to_string());

        assert_ne!(config.rpc_port, 9999);

        config.load_with_file();

        assert_eq!(config.db_path, "/tmp/custom_db");
        assert_eq!(config.rpc_port, 9999);
        assert_eq!(config.metrics_port, 7070);
    }
}
