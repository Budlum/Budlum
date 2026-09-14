# Bölüm 12: Production Hardening Durumu

Bu bölüm reponun güncel operasyonel gerçeklik tablosudur. Budlum Core kontrollü public-devnet adayıdır. Denetlenmiş Mainnet yazılımı değildir ve gerçek ekonomik değer taşımamalıdır.

## 0. v0.4 Protokol-Doğruluğu Düzeltmeleri

Settlement katmanının bağımsız bir mimari incelemesi, sadece bireysel bir hardening eksikliği değil, "deterministik settlement" iddiasının kendisini zayıflatan beş bulgu tespit etti. Beşi de v0.4'te kapatıldı:

| Bulgu | Düzeltme |
| --- | --- |
| Cross-domain settlement sırası ağ geliş sırasına bağlıydı, iki node farklı commitment'ları kabul edip ayrışabiliyordu | `settle_pending_domain_commitments()`, tüm domain'leri blok üretimi anında sabit artan `domain_id` sırasıyla işliyor (geliş sırasına göre değil); çakışmalar her node'da aynı şekilde çözülüyor (`src/chain/blockchain.rs`) |
| PoW/PoA/BFT finality adaptörleri kendi bildirilen confirmation/signer sayılarına güveniyordu | PoW artık gerçek bir mined header zinciri gerektiriyor (`PoWHeaderProof`); PoA/BFT artık PoS mekanizmasını yeniden kullanan gerçek bir BLS aggregate-signature quorum'u gerektiriyor (`src/domain/finality_adapter.rs`) |
| PoS/PoA/BFT domain'leri sıfır `validator_set_hash` ile kayıt olabiliyordu, bu da herhangi bir saldırgan-üretimi key setinin finality'yi geçmesine izin veriyordu | Kayıt artık quorum-imzalı domain'ler için sıfır validator_set_hash'i reddediyor (`validate_consensus_domain_registration`) |
| `DomainCommitment.state_updates`'in `state_root`'a hiçbir kriptografik bağı yoktu | `state_root` artık `state_updates` üzerinden bir Merkle kökü (`compute_state_updates_root`), kabul anında doğrulanıyor |
| `GlobalBlockHeader`'da gerçek bir account-state taahhüdü yoktu, `seal_global_header` hiçbir consensus round'u olmadan finality ima ediyordu | `global_state_root` (gerçek account state) ve `global_state_finalized` (dürüst BLS-finalization bayrağı) eklendi; özel bir settlement-seviyeli BFT round'u hâlâ açık (bkz. §5) |

Test sayısı: 342 → 346. Tam detay `SPECIFICATION.md` §1.4–1.5, §3.3, §6'da.

## 0b. v0.5 İkinci Tur Düzeltmeleri

Bağımsız bir ikinci inceleme turu üç bulgu daha tespit etti, v0.5'te kapatıldı:

| Bulgu | Düzeltme |
| --- | --- |
| `hash_to_g1`, `H(m) = scalar_hash(m) · G` hesaplıyordu — üreteçin herkese açık, deterministik bir skaler katı. Herhangi bir geçerli BLS imzası, secret key olmadan *tamamen farklı bir mesaj* için geçerli bir imzaya ölçeklenebiliyordu (`σ₂ = (s₂/s₁) · σ₁`) — protokoldeki her BLS-imzalı yapıya (finality sertifikaları, quorum sertifikaları) karşı evrensel bir sahtecilik | `hash_to_g1` artık `bls12_381`'in yerleşik RFC 9380 hash-to-curve'ünü kullanıyor (`ExpandMsgXmd<Sha256>` + SSWU); mesajları birbirleriyle veya üreteçle bilinen bir discrete-log ilişkisi olmayan noktalara eşliyor (`src/chain/finality.rs`) |
| §0'daki settlement determinizmi yalnızca "aynı kayıtlı domain verisi verildiğinde" geçerliydi — alıcı bir node settlement'ı kendi domain commitment görüşünden yeniden hesaplıyordu, bu da producer'dan farklı olabiliyor, geçerli blokların hatalı reddine veya `state_root` kontrolünün tek başına yakalayamayacağı bir ayrışmaya yol açabiliyordu | `Block` artık `settlement_watermarks` ve bunları blok hash'ine bağlayan bir `settlement_batch_root` taşıyor; `validate_and_add_block`, yerel olarak ne kadar veri mevcut olursa olsun bağımsızca yeniden hesaplamak yerine settlement'ı tam olarak bu watermark'larla sınırlı şekilde replay ediyor (`replay_settlement_to_watermarks`) (`src/chain/blockchain.rs`, `src/core/block.rs`) |
| `state_updates`/`state_root` iç tutarlılığı (§0/§1.5) gerçekti ama yetersizdi: finality proof'ları (PoW header binding, BLS quorum sertifikaları) yalnızca bir commitment'ın ham `domain_block_hash`'ine taahhüt ediyordu, hangi `state_root`'un onunla geldiğine değil — bu yüzden geçerli bir proof, farklı ama yine iç-tutarlı bir `state_updates` grubuna karşı prensipte replay edilebilirdi | `DomainCommitment::commitment_payload_hash()`, `state_root`'u (ve iddia edilen diğer tüm root'ları) PoW'un `extra` alanının ve BLS quorum sertifikasının `checkpoint_hash`'inin gerçekten bağlandığı şeyin içine katıyor (`src/domain/finality_adapter.rs`, `src/domain/types.rs`) |

Test sayısı: 346 → 351. Tam detay `SPECIFICATION.md` §1.6, §3.2.2, §3.3.4'te.

## 0c. v0.6 Üçüncü Tur Düzeltmeleri

Üçüncü bir inceleme turu yedi bulgu daha tespit etti, v0.6'da kapatıldı:

| Bulgu | Düzeltme |
| --- | --- |
| Güvenilmeyen bir `FinalityProof` içindeki `ValidatorSetSnapshot` olduğu gibi güveniliyordu — `FinalityCert::verify`, `set_hash`/`total_stake` alanlarını birbirleriyle karşılaştırıyordu, gerçek validator listesiyle değil; bu yüzden sahte bir `set_hash`, saldırgan kontrolündeki validator'larla eşleştirilip geçebiliyordu. `verify_pop()` vardı ama hiçbir yerde çağrılmıyordu | `ValidatorSetSnapshot::verify_self_consistent()`, `set_hash`/`total_stake`'i (overflow kontrollü) gerçek validator listesinden yeniden hesaplıyor ve kesin adres sıralaması (tekrarsız) zorunlu kılıyor; imzalayanlar PoP'tan geçmeli ve identity-point anahtar kullanamaz (`src/chain/finality.rs`) |
| Settlement replay canlı state/registry'yi doğrudan değiştiriyor ve domain cursor'larını ayrı, hata-yutan yazımlarla kaydediyordu — blok validasyonunun geri kalanı başarılı olsun ya da olmasın | Replay artık geçici state/registry kopyaları üzerinde çalışıyor; canlı state ve settle edilmiş domain cursor'ları yalnızca `commit_block_durable` başarılı olduktan sonra, bloğun aynı atomik storage batch'i içinde kaydediliyor (`src/chain/blockchain.rs`, `src/storage/db.rs`, `src/storage/traits.rs`) |
| Startup, hiçbir bloğa dahil edilmemiş olsa bile kayıtlı her domain commitment'ı koşulsuz uyguluyordu; reorg domain-registry settlement cursor'larını hiç yeniden kurmuyordu ve durable persistence'tan önce in-memory state'i değiştiriyordu | Startup, reorg ve snapshot yeniden kurma artık blok validasyonunun kullandığı aynı watermark-sınırlı `rebuild_state_and_registry`/`replay_settlement_to_watermarks` yolunu kullanıyor; reorg yeni zinciri in-memory'ye almadan önce durable olarak persist ediyor (`src/chain/blockchain.rs`) |
| Equivocation/tekrar kontrolleri ham `domain_block_hash`'i karşılaştırıyordu, bu yüzden aynı block hash'e ama farklı `state_root`'a sahip bir resubmission sessizce duplicate sayılabiliyordu | Her iki dedup kontrolü de artık `commitment_payload_hash()`'i karşılaştırıyor; `settlement_batch_root` uygulanan commitment'ların sıralı payload-hash'lerini kapsıyor; bir blok, bir domain'in zaten settle edilmiş height'ının altına gerileyen bir watermark iddia edemiyor (`src/chain/blockchain.rs`, `src/domain/types.rs`) |
| Gereken domain commitment'ı henüz gelmemiş bir blok, geçersiz bir blok gibi reddediliyor ve gönderen peer cezalandırılıyordu | `MissingDomainCommitment:` replay hatası bloğu atmak yerine kuyruğa alıyor (`pending_blocks`) ve otomatik olarak yeniden deniyor; network katmanı bu durum için artık peer'ı cezalandırmıyor (`src/chain/blockchain.rs`, `src/network/node.rs`) |
| `hash_to_g1` forgery regresyon testi gerçek tarihi exploit yerine keyfi yanlış bir skaler kullanıyordu, bu yüzden *o spesifik* saldırının kapatıldığını kanıtlamıyordu | Test artık tam eski skaler türetimini (SHA3-256 + domain tag) ve gerçek `s₂/s₁` oranını yeniden üretiyor — eski implementasyona karşı başarılı, güncel implementasyona karşı başarısız olduğu doğrulandı (`src/chain/finality.rs`) |
| `GlobalBlockHeader.global_state_root`, hangi bloğa ait olduğunu kaydetmeden bir bloğun state root'unu referans alıyordu, bu yüzden aynı root değerine ama farklı kaynağa sahip iki header ayırt edilemiyordu | `underlying_block_height`/`underlying_block_hash` eklendi, ikisi de header'ın kendi hash'ine besleniyor (`src/settlement/global_block.rs`, `src/chain/blockchain.rs`) |

Test sayısı: 351 → 359. Tam detay `SPECIFICATION.md` §1.7–1.9, §3.2.4–3.2.5, §3.5, §6.4'te.

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
| BLS Finality | `ValidatorKeys` içinde yeni `sign_bls()` / `verify_bls_sig()` primitifleriyle `BlsKeypair`. `ConsensusEngine::bls_secret_key()`, PoS engine üzerinden açığa çıkar. Validatörler BLS imzalı prevote/precommit mesajları üretir. Prevote quorum'a ulaşıldığında periyodik auto-precommit tetiklenir. `FinalityCert` doğrulaması BLS pairing ile yapılır. `hash_to_g1` artık gerçek RFC 9380 hash-to-curve kullanıyor (v0.5), sahteciliğe açık hash-sonra-çarp yapısı değil. `ValidatorSetSnapshot` self-consistency, PoP ve identity-key kontrolleri (v0.6) domain-seviyeli quorum sertifikalarındaki bir snapshot-spoofing sahteciliğini kapatıyor. |
| P2P Hardening | `p2p_identity_file` üzerinden kalıcı node kimliği (yükle-yoksa-üret deseni). Kalıcı peer ban'lar her 5 dakikada bir JSON'a yazılır ve başlangıçta yeniden yüklenir. mDNS politikası ağ bazlı `mdns_enabled` bayrağına uyar. `resolve_dns_seeds()` üzerinden DNS seed çözümlemesi. |

## 2. Aşamalı veya Kısmi İşler

| Alan | Sınır |
| --- | --- |
| Finality | Prevote/Precommit struct'ları, `FinalityAggregator`, sertifika üretimi ve BLS doğrulaması uygulandı ve test edildi. Validatörlerden BLS imzalı vote üretimi bağlandı: `sign_prevote()` ve `sign_precommit()` validatörün BLS secret key'ini kullanıyor. Periyodik voting loop otomatik olarak BLS prevote imzalayıp yayınlıyor; aggregator prevote quorum'u bildirdiğinde auto-precommit tetikleniyor. Saldırgan senaryolu çok-node finality testleri mevcut. Domain-seviyeli finality (yukarıdaki Budlum'un kendi zincir finality'sinden ayrı) artık PoW (mined header zinciri), PoA ve BFT (BLS quorum, PoS ile aynı mekanizma) için gerçek doğrulama yapıyor — bkz. §0. `ZkFinalityAdapter` hâlâ bir stub (3 hash'in sıfır olmadığını kontrol ediyor, gerçek proof doğrulaması yok). |
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

Güncel durum: **359 test.** `cargo clippy -D warnings`, CI'ın pinlediği Rust 1.94.0 toolchain'inde geçiyor; daha yeni bir yerel toolchain'de clippy'nin kendisi geliştiği için yeni uyarılar çıkabilir. Kritik adapter'lar tamamlanmadığı sürece Mainnet profili bilinçli olarak fail-closed davranır.

## 5. Mainnet v1 İçin Kalanlar

- Dış güvenlik denetimi
- Zamanlanmış backup restore tatbikatları (archive-node politikasının kendisi artık `archive_mode` ile uygulanmış durumda)
- Production runbook'ları ve incident response prosedürleri
- Her seal işleminin taze bir quorum sertifikası taşımasını zorunlu kılan settlement-seviyeli bir BFT round'u (`seal_global_header` şu an sadece iyi biçimlendirilmiş bir header istiyor — bkz. §0)
- Gerçek bir `ZkFinalityAdapter` (sıfır-olmayan hash kontrolü değil, gerçek bir finality-proving devresi)
- Gerçek bir foreign-chain PoW light client'ı (v0.4'teki header doğrulaması Budlum'un tanımladığı bir formatı kontrol ediyor, gerçek bir foreign chain'in header geçmişini değil)
- Eksik domain commitment'lar için aktif peer talebi: v0.6'daki retry kuyruğu (§0c) sıra dışı gelen gossip için peer cezalandırmasını durduruyor ve commitment'lar geldikçe otomatik yeniden deniyor, ama henüz özel bir "bana X commitment'ını ver" isteği göndermiyor — commitment'ın normal gossip yoluyla eninde sonunda gelmesine güveniyor.
- Budlum'un kendi validator'ları için on-chain BLS anahtar/PoP kaydı: hiçbir production kod yolu bir `AccountState` validator kaydına gerçek bir `bls_public_key`/`pop_signature` yazmıyor (yalnızca test setup'ları yazıyor) — bu yüzden Budlum'un kendi chain-seviyeli BLS checkpoint finality'sinin (§0c'de sertleştirilen domain-seviyeli quorum sertifikalarından ayrı olarak) gerçek validator'lar için hâlâ bir kayıt akışı yok. §0c'nin PoP kontrollerini sertleştirirken keşfedildi; bu tur kapsamı dışında.
