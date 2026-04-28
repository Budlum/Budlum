# Budlum Blok Zinciri Kitabı

Hoş geldiniz!

Bu kitap, **Budlum Blockchain** projesinin yaşayan teknik dokümantasyonudur. Ancak sıradan bir API referansı değildir. Bu kitap, **bir blok zinciri mimarının zihnine açılan bir kapıdır.**

Amacımız, sadece "Bu kod ne işe yarar?" sorusuna değil, **"Bu kodu neden böyle tasarladık?", "Hangi probleme çözüm ürettik?"** ve **"Alternatifleri nelerdi?"** sorularına cevap vermektir.

Kod tabanımız **Rust** ile yazılmıştır ve modern teknolojileri kullanır:
-   **Kriptografi:** Ed25519 ve BLS (Finalite), Merkle Patricia Trie (Hesap Durumu)
-   **Ağ:** Libp2p (Request-Response Sync, Gossipsub, Kademlia DHT)
-   **Veritabanı:** Sled (Gömülü Key-Value Store)
-   **Konsensüs:** Pluggable PoW, PoS ve Otomatik Finalite Oylama
-   **Gözlemlenebilirlik:** Prometheus Metrikleri (Grafana uyumlu)
-   **Yönetim:** TOML tabanlı esnek konfigürasyon sistemi

---

## Nasıl Okunmalı?

Kitap, "Mimarın Gözünden" (From the Architect's Eye) yaklaşımıyla 5 ana bölüme ayrılmıştır. Sırayla okumanızı tavsiye ederiz.

### 1. [Bölüm 1: Temeller ve Veri Yapıları](ch01_basics.md)
Blok zincirinin atomik parçaları.
-   [Bloklar](ch01_01_blocks.md): **Merkle Ağaçları** neden var? **SPV** (Light Client) nasıl çalışır?
-   [İşlemler](ch01_02_transactions.md): **Replay Attack** nedir? **Nonce** bunu nasıl engeller?
-   [Hesap Durumu](ch01_03_account_state.md): **State Machine** mantığı ve **UTXO vs Account** model karşılaştırması.

### 2. [Bölüm 2: Kriptografi](ch02_crypto.md)
Güvenliğin matematiksel temeli.
-   [İmzalar](ch02_01_signatures.md): Neden **Ed25519**? Deterministik imza nedir?
-   [Hash Ağaçları](ch02_02_merkle_trees.md): Veri bütünlüğü nasıl **O(log N)** maliyetle kanıtlanır?

### 3. [Bölüm 3: Konsensüs](ch03_consensus.md)
Merkeziyetsiz karar verme mekanizmaları.
-   [Motor Arayüzü](ch03_01_intro.md): **Modüler Mimari** ve `ConsensusEngine` Trait'i.
-   [Proof of Work](ch03_02_pow.md): Satoshi'nin vizyonu. **Zorluk Ayarlama Algoritması** (Difficulty Adjustment) analizi.
-   [Proof of Stake](ch03_03_pos.md): Modern çözüm. **Nothing at Stake** problemi, **Slashing** (Ceza) ve **Lider Seçimi**.

### 4. [Bölüm 4: Ağ ve P2P](ch04_networking.md)
Bilgisayarların ortak dili.
-   [Node Mimarisi](ch04_01_node.md): **Tokio Event Loop** ve Asenkron programlama.
-   [Peer Manager](ch04_02_peer_manager.md): **Oyun Teorisi** ile itibar yönetimi ve **Sybil Saldırısı** koruması.
-   [Protokol](ch04_03_protocol.md): **Protobuf** ağ mesajları, deterministik iç serileştirme ve ağ limitleri.

### 5. [Bölüm 5: Depolama ve Verim](ch05_storage.md)
Verinin kalıcılığı.
-   [Veritabanı](ch05_01_storage.md): **Sled** tabanlı kalıcı depolama, migration ve snapshot export.
-   [Mempool](ch05_02_mempool.md): **Ücret Piyasası** (Fee Market).
-   [Snapshot](ch05_03_snapshots.md): **Pruning** (Budama).

### 6. [Bölüm 6: JSON-RPC API](ch06_json_rpc.md)
Dış dünya ile entegrasyon. **Budlum Standardı** (`bud_`) metotları ve kullanım rehberi.

### 7. [Bölüm 7: Ağ Ayrımı](ch07_network_distinctions.md)
**Mainnet, Testnet ve Devnet** yapılandırmaları. CLI tabanlı ağ seçimi ve izolasyon.

### 8. [Bölüm 8: Kaos Mühendisliği](ch08_chaos_engineering.md)
Ağ dayanıklılığı için **Kaos Testleri**, re-org koruması ve hata simülasyonları.

### 9. [Bölüm 9: Dizin Yapısı](ch09_directory_structure.md)
Yeni **Katmanlı Modüler Mimari** ve dosya düzeni.

---

## Katkıda Bulunun

Budlum açık kaynaklı bir projedir. Kodları `infra/src` altında bulabilir, bu kitaptaki teorileri pratikle birleştirebilirsiniz. Bir hata görürseniz veya daha iyi bir açıklamanız varsa, lütfen katkıda bulunun!

İyi okumalar,
*Budlum Çekirdek Ekibi*
