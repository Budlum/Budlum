# Bölüm 4.3: Ağ Protokolü ve Mesajlaşma

Bu bölüm, makinelerin birbirleriyle konuşurken kullandığı dili (`NetworkMessage`) ve verilerin kablodan geçmeden önce nasıl 0 ve 1'lere dönüştürüldüğünü (Serialization) analiz eder.

Kaynak Dosya: `src/network/protocol.rs` (Varsayımsal)

---

## 1. Veri Yapıları: Ortak Dil

Dünyanın her yerindeki bilgisayarların anlaşabilmesi için ortak bir `Enum` tanımlarız.
# Protokol Mesajları ve Serileştirme (Serialization)

`budlum-core` içindeki node'lar, eşler düzeyindeki (p2p) ağı yönetmek ve veri paylaşmak için özel `NetworkMessage` protokolünü kullanırlar.

## `NetworkMessage` Neler İçerir?

Ağdaki tüm iletişim bir enum (numaralandırılmış yapı) üzerinden geçer. En önemli türleri şunlardır:

1.  **El Sıkışma (Handshake / HandshakeAck)**: Ağa yeni katılanlar bağlanırken versiyon ve `chain_id` bilgilerini doğrularlar. **Hardening Phase 2** ile artık `validator_set_hash` (aktif validatörlerin özeti) ve `supported_schemes` (ED25519, BLS, DILITHIUM) bilgileri de doğrulanır. Uyumsuz olanlar anında engellenir.
2.  **Block**: Yeni çıkarılan bir bloğun tüm peer'lara (eşlere) yayılması.
3.  **Transaction**: Yeni işlemlerin yayılması.
4.  **Finalite Oyları (Prevote / Precommit)**: BLS tabanlı finalite katmanı oyları.
5.  **FinalityCert**: Bir checkpoint'in finalize edildiğini kanıtlayan eşik imzalı sertifika.
6.  **QC İstekleri (GetQcBlob / QcBlobResponse)**: Optimistik QC doğrulaması için Dilithium imzalı blob paketlerinin paylaşımı.
7.  **NewTip / Sync Mesajları**: Zincir senkronizasyonu için kullanılan `GetBlocksByHeight` vb. mesajlar.

*Tam Liste kaynak kodu üzerinden incelenebilir: `src/network/protocol.rs`*

## GossipSub ile Yayın Yapma (Publish)

Budlum Core iletişimi **GossipSub** üzerinden yürütür. Ancak her veri Gossip ile yayılmaz:
- **Konu Başlıkları (Topics):** "blocks" ve "transactions" gibi veriler tüm ağa duyurulur.
- **Limitler:** GossipSub saniyeler içinde ulaşır ama yüksek bant genişliği harcar.

## Request-Response Senkronizasyonu

**Production Hardening** aşamasında, büyük veri transferleri (geçmiş blokları indirme) GossipSub üzerinden değil, doğrudan birebir (Peer-to-Peer) **Request-Response** protokolü ile yapılır.

- **Protokol Kimliği:** `/budlum/sync/1.0.0`
- **SyncCodec:** Veriyi `length-prefixed` (uzunluk ön ekli) şekilde serileştiren ve akış (stream) üzerinden güvenli aktaran özel bir yapı.
- **Aktör Entegrasyonu:** `Node` asenkron döngüsü, gelen istekleri `ChainActor`'a iletir; böylece blok ve headerlar kilitlenme (lock) olmadan yüksek hızda servis edilir.
- **Neden?** GossipSub "herkese bağırır". Senkronizasyonda ise sadece bir kişiye "bana X ile Y arasındaki blokları ver" deriz. Bu ağ trafiğini %90 azaltırken, senkronizasyon hızını 5-10 kat artırır.

## Serileştirme (Serialization)

Mesajlar ağ üzerine bayt olarak çıkmadan önce serileştirilir.
`budlum-core`, **Hardening Phase 2** ile birlikte hibrit bir serileştirme kullanır:
- **Protobuf (`protocol.proto`):** Yüksek performanslı ağ mesajları ve ana veri yapıları için kullanılır (CPU ve bant genişliği tasarrufu).
- **Serde-JSON:** Bazı yüksek seviye konfigürasyon ve tanı mesajları için (okunabilirlik amacıyla) kullanılır.
- **Bincode:** Slashing kanıtları gibi deterministik (bayt-bayt aynı) olması gereken yapılar için tercih edilir.

**Neden Protobuf?**
Blok zincirinde saniyede binlerce işlem olur. JSON kullanmak, ağı %30-40 yavaşlatır ve CPU'yu yorar. Protobuf, veriyi ikili (binary) formatta paketleyerek çok daha hızlı ve küçük paketler oluşturur.

---

## 3. Limitler ve Güvenlik

Ağdan gelen veri güvenilmezdir. Biri size 10 GB'lık tek bir mesaj yollayıp RAM'inizi patlatabilir (Memory Exhaustion Attack).

**Kod (Gossipsub Config):**
```rust
// libp2p ayarlarında
let gossipsub_config = GossipsubConfigBuilder::default()
    .max_transmit_size(1024 * 1024) // 1 MB Limit
    .build()
    .unwrap();
```

Bu limit sayesinde, 1 MB'tan büyük bloklar veya mesajlar ağda otomatik olarak reddedilir. Bu bir konsensüs kuralıdır. Eğer blok boyutunu artırmak isterseniz, tüm ağın yazılımını güncellemesi (Hard Fork) gerekir.
