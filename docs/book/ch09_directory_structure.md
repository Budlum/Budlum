# Bölüm 9: Dizin Yapısı ve Modülerlik

Budlum projesi, büyüklüğü arttıkça karmaşıklığı yönetmek adına **katmanlı bir mimariye** geçirilmiştir.

## 1. Mevcut Dizin Yapısı

```text
src/
├── main.rs                 # Uygulama giriş noktası (CLI & Service Runner)
├── lib.rs                  # Modüllerin public dışa aktarımı
├── cli/                    # CLI komutları ve argüman yönetimi
├── core/                   # Temel veri yapıları (Block, Tx, Account, Config)
├── chain/                  # Zincir mantığı (Blockchain, Genesis, Snapshots)
├── network/                # P2P altyapısı (Node, PeerManager, Protocol)
├── rpc/                    # JSON-RPC sunucusu ve API tanımları
├── storage/                # Veritabanı (RocksDB/DumbDB) katmanı
├── execution/              # İşlem yürütme (Executor) ve State geçişleri
├── consensus/              # Konsensüs algoritmaları (PoW, PoA, PoS, Finality)
├── mempool/                # İşlem havuzu (Mempool) yönetimi
└── tests/                  # Entegrasyon ve Kaos testleri
```

## 2. Modülerlik Kuralları

- **Core Üzerinde Bağımlılık Yok**: `core/` modülü en alttadır ve projenin geri kalanından bağımsızdır. Sadece temel tipleri (Block, Transaction) içerir.
- **Ayrık Konsensüs**: Konsensüs algoritmaları (`consensus/`) birer "Plugin" gibi çalışır. Blockchain'e enjekte edilebilirler.
- **İletişim Kanalı (MPSC)**: Modüller birbirini doğrudan çağırmak yerine çoğunlukla mesaj kuyrukları (Channel) üzerinden asenkron konuşur.

## 3. Geliştirici Deneyimi

Bu yapı sayesinde, yeni bir konsensüs algoritması eklemek isteyen bir geliştirici, sadece `consensus/` ve `core/block.rs` üzerinde değişiklik yaparak diğer modülleri etkilemeden ilerleyebilir.
