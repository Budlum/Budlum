# Bölüm 9: Dizin Yapısı ve Modülerlik

Budlum projesi, büyüklüğü arttıkça karmaşıklığı yönetmek adına **katmanlı bir mimariye** geçirilmiştir.

## 1. Mevcut Dizin Yapısı

```text
config/
├── mainnet.toml            # Mainnet node profile; real bootnodes required before launch
├── testnet.toml            # Public testnet profile
└── devnet.toml             # Local development profile

src/
├── main.rs                 # Uygulama giriş noktası (CLI & Service Runner)
├── lib.rs                  # Modüllerin public dışa aktarımı
├── cli/                    # CLI komutları ve argüman yönetimi
├── core/                   # Temel veri yapıları (Block, Tx, Account, Config, Metrics)
├── chain/                  # Zincir mantığı (Blockchain, Actor, Genesis, Snapshots)
│   └── chain_actor.rs       # Zincir durumunu yöneten Actor ve Handle tanımları
├── network/                # P2P altyapısı (Node, PeerManager, Protocol, SyncCodec)
│   └── sync_codec.rs        # P2P senkronizasyon için özel veri kodlayıcı
├── rpc/                    # JSON-RPC sunucusu ve API tanımları
├── storage/                # Sled veritabanı, migration ve snapshot export katmanı
├── execution/              # İşlem yürütme, State geçişleri ve BudZKVM backend
│   ├── executor.rs         # Transfer/Stake/Vote/ContractCall state transition mantığı
│   └── zkvm.rs             # BudZKVM bytecode decode, VM execution, proof verify
├── consensus/              # Konsensüs algoritmaları (PoW, PoA, PoS, Finality)
├── mempool/                # İşlem havuzu (Mempool) yönetimi
└── tests/                  # Doğrulama, Kaos ve Performans testleri
    ├── integration.rs      # Uçtan uca sistem testleri
    ├── chaos.rs            # Ağ bölünmesi ve hata simülasyonları
    ├── hardening.rs        # Güvenlik ve kaynak sınırı testleri
    ├── zkvm.rs             # ContractCall ve BudZKVM execution güvenlik testleri
    └── bench_performance.rs # High-TPS performans ölçüm aracı
```

## 2. Modülerlik Kuralları

- **Core Üzerinde Bağımlılık Yok**: `core/` modülü en alttadır ve projenin geri kalanından bağımsızdır. Sadece temel tipleri (Block, Transaction) içerir.
- **Ayrık Konsensüs**: Konsensüs algoritmaları (`consensus/`) birer "Plugin" gibi çalışır. Blockchain'e enjekte edilebilirler.
- **İletişim Kanalı (MPSC)**: Modüller birbirini doğrudan çağırmak yerine çoğunlukla mesaj kuyrukları (Channel) üzerinden asenkron konuşur.
- **Ağ Profilleri**: Mainnet, testnet ve devnet parametreleri `src/core/chain_config.rs` içinde merkezi olarak tanımlanır; TOML config dosyaları operatör değerlerini taşır.
- **Genesis İzolasyonu**: `src/chain/genesis.rs` her ağ için ayrı genesis config üretir. Mainnet/testnet/devnet chain ID, validator set, allocation, reward, gas schedule ve timestamp ayrıdır.
- **Execution Backend Ayrımı**: `src/execution/executor.rs` L1 state transition kapısıdır; BudZKVM'e özel bytecode/proof mantığı `src/execution/zkvm.rs` içinde izole edilir. Böylece transfer/stake/vote yolları contract execution bağımlılıklarıyla karışmaz.

## 3. Geliştirici Deneyimi

Bu yapı sayesinde, yeni bir konsensüs algoritması eklemek isteyen bir geliştirici, sadece `consensus/` ve `core/block.rs` üzerinde değişiklik yaparak diğer modülleri etkilemeden ilerleyebilir.

Yeni bir ağ profili eklemek için güncellenmesi gereken ana noktalar:

1. `src/core/chain_config.rs`: Chain ID, port, consensus, gas, mempool ve security değerleri.
2. `src/chain/genesis.rs`: Ağın genesis allocation ve validator set değerleri.
3. `config/<network>.toml`: Operatör config dosyası.
4. `docs/book/ch07_network_distinctions.md`: Ağ dokümantasyonu.
