# Bölüm 12: Production Hardening Durumu

Bu bölüm reponun güncel operasyonel gerçeklik tablosudur. Budlum Core kontrollü public-devnet adayıdır. Denetlenmiş Mainnet yazılımı değildir ve gerçek ekonomik değer taşımamalıdır.

## 1. Uygulanan Korumalar

| Alan | Güncel davranış |
| --- | --- |
| Konfigürasyon | Katı Config V2 bilinmeyen alanları, profil/chain-ID uyumsuzluğunu, güvensiz Mainnet feature flag'lerini, eksik Mainnet genesis'i, boş Mainnet seed ayarını ve Mainnet mDNS kullanımını reddeder. |
| Genesis | `genesis build` private key materyalini yazdırmaz. Otomatik allocation anahtarı yalnız devnet için ve açık output dosyasıyla üretilebilir. Devnet dışı genesis açık validatör listesi ister. |
| Başlangıç | Storage açılış hataları başlangıcı durdurur. Mevcut DB ayarlanan genesis kimliğine karşı kontrol edilir. Özel genesis dosyası parse edilmeli ve seçili chain ID ile eşleşmelidir. |
| State commitment | `ConsensusStateV2` hesapları, validatörleri, unbonding kuyruğunu, ekonomiyi, bridge, message, settlement ve global-header özet state'ini bağlar. |
| Kalıcılık | Canonical değişiklikler `IN_PROGRESS_HEIGHT` recovery marker'ı ve atomik Sled batch içeren `DurableCommitBatch` kullanır. |
| Snapshot aşaması | Snapshot dosyaları sayısal sıralanır; bozuk en yeni dosya karantinaya alınır. `StateSnapshotV2`, tam konsensüs metadata'sıyla (epoch, fee, ödül, cross-domain root'lar, unbonding kuyruğu, finality sertifikaları) canonical runtime formatıdır. |
| RPC | Ayrı public ve operator HTTP listener'ları. Public: API-key auth, CORS allowlist, per-IP rate limiting, trusted-proxy doğrulama, 10MB body limiti, 500 max bağlantı. Operator: yalnızca localhost, auth yok, 50MB body limiti. `bud_health` ve `bud_nodeInfo` endpoint'leri. |
| CI | GitHub Actions Rust `1.94.0` sürümünü pinler; format, `cargo check`, warning'leri reddeden Clippy, workspace testleri ve `--release --locked` build çalıştırır. |
| PKCS#11 | `ConsensusSigner` trait + `Pkcs11Signer` adaptörü (`cryptoki` ile) + `KeyPairSigner` local fallback. `ConsensusEngine` trait `fn signer()` sunar. Blok imzalama HSM varsa onu, yoksa local dosyayı kullanır. |
| BLS Finality | `ValidatorKeys` içinde yeni `sign_bls()` / `verify_bls_sig()` primitifleriyle `BlsKeypair`. `ConsensusEngine::bls_secret_key()`, PoS engine üzerinden açığa çıkar. Validatörler BLS imzalı prevote/precommit mesajları üretir. Prevote quorum'a ulaşıldığında periyodik auto-precommit tetiklenir. `FinalityCert` doğrulaması BLS pairing ile yapılır. |
| P2P Hardening | `p2p_identity_file` üzerinden kalıcı node kimliği (yükle-yoksa-üret deseni). Kalıcı peer ban'lar her 5 dakikada bir JSON'a yazılır ve başlangıçta yeniden yüklenir. mDNS politikası ağ bazlı `mdns_enabled` bayrağına uyar. `resolve_dns_seeds()` üzerinden DNS seed çözümlemesi. |

## 2. Aşamalı veya Kısmi İşler

| Alan | Sınır |
| --- | --- |
| Finality | Prevote/Precommit struct'ları, `FinalityAggregator`, sertifika üretimi ve BLS doğrulaması uygulandı ve test edildi. Validatörlerden BLS imzalı vote üretimi bağlandı: `sign_prevote()` ve `sign_precommit()` validatörün BLS secret key'ini kullanıyor. Periyodik voting loop otomatik olarak BLS prevote imzalayıp yayınlıyor; aggregator prevote quorum'u bildirdiğinde auto-precommit tetikleniyor. Saldırgan senaryolu çok-node finality testleri mevcut. |
| P2P | Version ve chain ID zorlanıyor. Kalıcı kimlik, kalıcı ban'lar, mDNS politikası ve DNS seed çözümlemesi runtime'da bağlı. Validator-set hash ve desteklenen-scheme politikası beklemede. |
| RPC | Config, public/operator listener ve trusted proxy alanlarını parse ediyor. Runtime iki ayrı sunucu başlatıyor (public + operator). `is_ip_allowed`, trusted-proxy doğrulamasını uyguluyor: yalnızca yapılandırılmış proxy IP'lerinden gelen istekler `X-Forwarded-For` kullanabiliyor. `is_per_ip_rate_limited`, IP başına sliding-window kota uyguluyor (`src/rpc/server.rs`). Health ve node-info endpoint'leri canlı. `bud_adminBanPeer`/`bud_adminUnbanPeer`/`bud_adminListBannedPeers` yalnızca operator listener'da çalışıyor; `require_operator()` bunları public listener'da reddediyor. |
| Metrics | Prometheus tanımları ve endpoint mevcut. Canlı collector'lar bağlı: `budlum_chain_height`, `budlum_finalized_height`, `budlum_finality_lag`, `budlum_blocks_produced`, `budlum_transactions_processed`, `budlum_reorgs_total`, `budlum_mempool_size`, `budlum_mempool_evictions`, `budlum_mempool_expired_cleanups`, `budlum_p2p_messages_received`, `budlum_p2p_peers_connected`. Histogram'lar da artık gerçekten besleniyor: `block_propagation_seconds` (gossip ile blok alındığında, `src/network/node.rs`), `consensus_round_seconds` (her `produce_block()` çağrısında, `src/chain/blockchain.rs`), `storage_write_seconds` (her durable commit batch'te), `storage_read_seconds` (`get_transaction_by_hash`/`get_transaction_receipt` içindeki `get_tx_block_height` sorgusunda). |
| Snapshot V2 | V2, canonical runtime formatı. `AccountState::from_snapshot_v2()`, tüm konsensüs metadata'sını (epoch, base_fee, block_reward, unbonding_queue, cross-domain root'lar, finality sertifikaları) geri yüklüyor. `Blockchain` başlangıçta önce V2, sonra V1 deniyor. P2P snapshot'ları geriye dönük uyumlu taşıma için V2 verisini gömüyor. `apply_state_snapshot` önce V2 restore'u deniyor. `session_id` üzerinden chunk-session bağlama, oturumlar arası chunk karışmasını engelliyor. Replay eşdeğerliği doğrulandı. `PruningManager::with_archive_mode(true)` (config: `features.archive_mode`) ile `get_prunable_blocks` her zaman boş dönüyor — archive node snapshot almaya devam ederken tam blok geçmişini koruyor. Zamanlanmış backup restore tatbikatları hâlâ operasyonel bir konu, kod değil. |
| Storage | Durable block commit mevcut. `Storage::run_migrations`, gerçek sıralı bir executor: kayıtlı `(to_version, fn)` adımlarını sırayla uyguluyor, her adımdan sonra `SCHEMA_VERSION`'ı damgalıyor (böylece migration ortasında çökme olursa doğru yerden devam edilir), ve DB'nin kayıtlı versiyonu binary'nin desteklediğinden yeniyse açmayı reddediyor. `CURRENT_SCHEMA_VERSION` hiç değişmediği için henüz kayıtlı bir migration adımı yok — executor'ın kendisi sentetik adımlarla test edildi (`src/storage/db.rs`). Gerçek bir şema değişikliği geldiğinde yayınlanacak migration *adımları* ve restore tatbikatları hâlâ eksik. |
| Deployment | Docker multi-stage image, Prometheus'lu 4-node docker-compose devnet, systemd unit dosyası ve fuzz harness'leri mevcut. Release ceremony kayıtları ve incident runbook'ları eksik. |

## 3. Açık Mainnet Engelleri

1. ~~PKCS#11 konsensüs signer adapter'ını uygula ve denetle.~~ **TAMAMLANDI (v0.2-dev)**
2. ~~BLS imzalı prevote/precommit vote üretimini, canlı sertifika yayınını ve saldırgan çok node'lu finality testlerini tamamla.~~ **TAMAMLANDI (v0.3-dev):** `BlsKeypair`, `sign_bls()`, BLS imzalı prevote/precommit, prevote quorum'da auto-precommit, saldırgan equivocation, sahte agregat imza ve tam 4-validatörlü finality akışı dahil 12 BLS/finality testi.
3. ~~Public ve operator RPC sunucularını ayır, trusted proxy zorla, health endpoint ekle, connection/body limitleri, istemci bazlı kota ve operator-only admin metodları tanımla.~~ **TAMAMLANDI (v0.3-dev):** `RpcMode::Public`/`RpcMode::Operator` üzerinden çift `RpcServer`, `main.rs`'te ayrı listener'lar, trusted-proxy `is_ip_allowed`, `bud_health`/`bud_nodeInfo` endpoint'leri, `max_request_body_size` (10MB/50MB), `max_connections` (500/10), per-IP rate limiting, `require_operator()` ile korunan `bud_adminBanPeer`/`bud_adminUnbanPeer`/`bud_adminListBannedPeers`.
4. ~~Kalıcı P2P kimliği, discovery politikası, DNS seed ve kalıcı peer ban bağlantılarını tamamla.~~ **TAMAMLANDI (v0.3-dev):** `load_or_generate_identity_key()`, `with_identity()`/`with_banned_peer_db()`/`with_dns_seeds()` builder'ları, kalıcı ban JSON persistence, `Network::security_config()` üzerinden mDNS politikası, DNS seed çözümleme ve dial etme.
5. ~~Snapshot V2 restore, kimliği doğrulanmış dağıtım, chunk-session bağlama, replay eşdeğerliği, backup restore tatbikatı ve archive politikasını tamamla.~~ **TAMAMLANDI (v0.3-dev):** `AccountState::from_snapshot_v2()`, `PruningManager`'da canonical format olarak V2, `Blockchain` başlangıcında V2-öncelikli restore, P2P V2 taşıma, chunk `session_id` bağlama, tam metadata + finality sertifikası restore'u yapan `apply_v2_snapshot()`, doğrulanmış replay eşdeğerliği, budamayı tamamen kapatan `archive_mode` bayrağı. Zamanlanmış backup restore tatbikatları hâlâ eksik (operasyonel, kod değil).
6. Governance, BudZKVM contract ve pruning özelliklerini ayrı incelemeler tamamlanana kadar Mainnet v1 için kapalı tut.
7. ~~Deployment paketleri, release ceremony kayıtları, dashboard'lar, incident runbook'ları, fault injection, fuzzing sonuçları ve dış güvenlik denetimi üret.~~ **KISMEN (v0.3-dev):** Docker multi-stage image, Prometheus'lu 4-node docker-compose, systemd unit, 4 cargo-fuzz hedefi, canlı Prometheus collector bağlantısı. Dış denetim ve production runbook'ları eksik.

## 4. Release Kapıları

Her release adayı şu komutları geçmelidir:

```bash
nix develop --command cargo fmt --all -- --check
nix develop --command cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop --command cargo test --workspace
nix develop --command cargo build --release --locked
git diff --check
```

Güncel durum: **342 test.** `cargo clippy -D warnings`, CI'ın pinlediği Rust 1.94.0 toolchain'inde geçiyor; daha yeni bir yerel toolchain'de clippy'nin kendisi geliştiği için yeni uyarılar çıkabilir. Kritik adapter'lar tamamlanmadığı sürece Mainnet profili bilinçli olarak fail-closed davranır.

## 5. Mainnet v1 İçin Kalanlar

- Dış güvenlik denetimi
- Zamanlanmış backup restore tatbikatları (archive-node politikasının kendisi artık `archive_mode` ile uygulanmış durumda)
- Production runbook'ları ve incident response prosedürleri
