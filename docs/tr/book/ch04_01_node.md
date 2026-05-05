# Bölüm 4.1: Node Mimarisi ve Olay Döngüsü

Bu bölüm, ağın omurgasını oluşturan `Node` yapısını, `libp2p` entegrasyonunu ve asenkron (async) olay döngüsünü satır satır inceler.

Kaynak Dosya: `src/network/node.rs`

---

## 1. Veri Yapıları: Bağlantı Noktası

Bir "Node" (Düğüm), hem blok zincirini yöneten hem de diğer bilgisayarlarla konuşan birimdir.

### Struct: `BudlumBehaviour`

Libp2p kütüphanesi "Modüler Ağ Davranışları" kullanır. Bizim düğümümüzün yetenekleri burada tanımlanır.

**Kod:**
```rust
#[derive(NetworkBehaviour)]
pub struct BudlumBehaviour {
    pub gossipsub: gossipsub::Behaviour, // Radyo Yayını (Blok/Tx Duyurusu)
    pub mdns: mdns::tokio::Behaviour,    // Yerel Ağ Keşfi (LAN)
    pub identify: identify::Behaviour,   // Kimlik Kartı (Version Info)
    pub kad: Kademlia<MemoryStore>,      // Telefon Rehberi (DHT - Peer Discovery)
    pub ping: ping::Behaviour,           // Nabız Kontrolü
}
```

**Analiz:**

| Davranış (Behaviour) | Protokol | Ne İşe Yarar? |
| :--- | :--- | :--- |
| `gossipsub` | **PubSub** | **Dedikodu Protokolü.** "Bende yeni blok var!" dediğinizde, bunu komşularınıza, onlarında komşularına iletmesini sağlar. Blok ve işlem yayılımı bununla yapılır. |
| `mdns` | **mDNS** | **Otomatik Keşif.** Aynı Wi-Fi'daki diğer Budlum node'larını otomatik bulur. Evde test yaparken IP girmek zorunda kalmazsınız. |
| `kad` | **Kademlia DHT** | **Dağıtık Rehber.** İnternetin öbür ucundaki bir Node'u bulmak için kullanılır. Merkezi sunucu (Tracker) yoktur. Herkes rehberin bir sayfasını tutar. |
| `identify` | **Identify** | **Versiyon Kontrolü.** Bağlandığınız kişiye "Ben Budlum v1.0, Rust ile yazıldım" dersiniz. Uyumsuz versiyonlar birbirini reddeder. |

---

### Struct: `Node`

**Kod:**
```rust
pub struct Node {
    swarm: Swarm<BudlumBehaviour>, // Ağ Motoru
    command_rx: mpsc::Receiver<NodeCommand>, // İçerden gelen emirler
    command_tx: mpsc::Sender<NodeCommand>, // Emir gönderme kanalı
    pub peer_id: PeerId,
    pub chain: ChainHandle, // Zincir Aktörü ile iletişim (Async)
    pub peer_manager: Arc<Mutex<PeerManager>>, // Eş Yönetimi
    pub bootstrap_peers: Vec<String>,
    pub max_peers: usize,
    // ...
}
```

### Struct: `NodeClient`

Dış modüllerin (örn: RPC Sunucusu) Düğüm ile güvenli bir şekilde konuşmasını sağlayan hafif bir "kumanda" yapısıdır.

```rust
pub struct NodeClient {
    sender: mpsc::Sender<NodeCommand>,
    pub peer_id: PeerId,
}
```

**Tasarım Kararı: Actor Model (ChainActor & ChainHandle)**
-   **Lockless Design:** Eskiden kullanılan `Arc<Mutex<Blockchain>>` yerine artık **Actor Model** kullanılmaktadır. 
-   **ChainActor:** Zincir verisinin (Blockchain, State, Mempool) tek sahibidir. Kendi thread'inde çalışır ve gelen mesajları (ChainCommand) sırayla işler.
-   **ChainHandle:** Diğer modüllerin (Node, RPC) zincire erişmek için kullandığı asenkron kumandadır. Bu sayede ağ trafiği işlenirken veritabanı kilitlenmesi (lock contention) yaşanmaz.

---

## 2. Olay Döngüsü (The Event Loop)

Düğüm çalıştığı sürece (`run` fonksiyonu), hiç durmayan bir döngü içindedir.

```rust
pub async fn run(&mut self) {
    let mut gc_interval = tokio::time::interval(Duration::from_secs(60));
    let mut discovery_interval = tokio::time::interval(Duration::from_secs(300));
    
    loop {
        tokio::select! {
            // DURUM 1: Arka Plan Bakım Görevleri (Background Maintenance)
            _ = gc_interval.tick() => {
                // Her 60 saniyede bir Mempool'daki süresi dolmuş işlemleri (TTL) sil 
                // ve PeerManager'daki yasak süresi dolmuş eşlerin (Bans) engelini kaldır.
            }
            _ = discovery_interval.tick() => {
                // Her 5 dakikada bir Kademlia DHT ağında yeni eşler (Peers) ara.
            }

            // DURUM 2: Ağdan bir olay geldi (Dış dünya)
            event = self.swarm.select_next_some() => {
                self.handle_network_event(event).await;
            }

            // DURUM 3: İçerden bir komut geldi (İç dünya)
            command = self.command_rx.recv() => {
                if let Some(cmd) = command {
                    self.handle_command(cmd).await;
                }
            }

            // DURUM 4: Arka Plan Görevleri
            _ = finality_interval.tick() => {
                // Her 30 saniyede bir Checkpoint yüksekliğinde oylama başlat.
            }
        }
    }
}
```

**Analiz: `tokio::select!`**
Bu makro, Go dilindeki `select` gibidir. Budlum'da artık daha zengin bir olay döngüsü vardır:
-   **Maintenance:** TTL süresi dolan işlemleri siler.
-   **Finality:** Periyodik olarak checkpoint oylaması (Prevote) yapar.
-   **Metrics:** Prometheus üzerinden düğüm sağlığını dışarı sunar.
-   **QC Recovery:** Bir `FinalityCert` gelir ama ilgili `QC_BLOB` yerelde yoksa, node sertifikayı pending kuyruğuna alır ve otomatik olarak `GetQcBlob` isteği başlatır. Blob import edildiğinde pending sertifika tekrar işlenir.
-   **Slashing Evidence Gossip:** Node, yerelde tespit edilen slashing kanıtlarını periyodik olarak drain eder ve `NetworkMessage::SlashingEvidence` şeklinde ağa yayar.

---

## 3. Çatal Seçimi (Fork-Choice) ve Reorg

Budlum **Hardening** ile birlikte artık daha akıllı bir blok işleme mantığına sahiptir. Bir blok geldiğinde 3 senaryo işletilir:

1. **Sıralı Blok (Normal):** Gelen bloğun indeksi, bizim zincir uzunluğumuza eşitse direkt doğrulanır ve eklenir.
2. **Çatal (Fork):** Gelen bloğun indeksi bizimkinden küçükse ama hash farklıysa, ağda bir bölünme olduğu anlaşılır.
    - `try_reorg` fonksiyonu tetiklenir.
    - Eğer gelen çatalın toplam zorluğu (veya uzunluğu) bizimkinden fazlaysa, eski state geri sarılır (Revert) ve yeni çatala geçilir.
3. **Senkronizasyon (Sync Request):** Eğer gelen blok çok ilerideyse (bizde aradaki bloklar yoksa), otomatik olarak `GetHeaders` isteği atılarak senkronizasyon başlatılır.
4. **Handshake Height Gap:** Peer handshake sırasında bizden yüksek `best_height` bildirirse, node blok trafiğini beklemeden headers-first sync başlatır.

`bud_syncing` artık hardcoded `false` dönmez; node'un gerçek sync durumunu raporlar.

---

### Fonksiyon: `handle_network_event`

Ağdan gelen paketleri açtığımız yer.

```rust
async fn handle_network_event(&mut self, event: SwarmEvent<BudlumBehaviourEvent>) {
    match event {
        // Yeni bir Blok veya İşlem geldiğinde (Gossipsub)
        SwarmEvent::Behaviour(BudlumBehaviourEvent::Gossipsub(gossip_event)) => {
            if let GossipsubEvent::Message { message, .. } = gossip_event {
                // Mesajı Protobuf üzerinden ayrıştır ve boyut limitlerini uygula.
                let network_msg = NetworkMessage::from_bytes_validated(&message.data)?;
                
                match network_msg {
                    NetworkMessage::Block(block) => {
                        tracing::info!(height = block.index, "new block received");
                        self.process_incoming_block(block).await;
                    }
                    NetworkMessage::Transaction(tx) => {
                        // Mempool'a ekle
                        self.blockchain.lock().unwrap().add_transaction(tx);
                    }
                    // ...
                    // ...
                }
            }
        }
    }
}
```

**Analiz: Peer Count Takibi ve Ağ Limitleri**
`run` döngüsü içinde `SwarmEvent::ConnectionEstablished` olduğunda `peer_count` artırılır, `ConnectionClosed` olduğunda azaltılır. Bu veri atomik olduğu için RPC sunucusu tarafından kilitlenme (lock) gerektirmeden anlık okunabilir.

`Node::apply_network_security(network)` çağrısı, `src/core/chain_config.rs` içindeki network-specific `SecurityConfig` değerlerini node'a uygular. Böylece mainnet/testnet/devnet için maksimum peer limiti ayrı çalışır.
        
        // Yeni biri bağlandığında (Connection Established)
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            tracing::info!(%peer_id, "peer connected");
            // Onu tanımak için Kademlia'ya ekle
            self.swarm.behaviour_mut().kad.add_address(&peer_id, ...);
        }
        
        // ...
    }
}
```

---

## 4. Bakım ve Operasyon (Hardening)

Budlum'un üretim sürümünde, düğüm operatörleri için yeni bakım komutları eklenmiştir:

- **`--check-db`**: Düğümü başlatmadan önce veritabanındaki tüm blokların hash ve bağlantı (linkage) bütünlüğünü kontrol eder.
- **`--repair-db`**: Eğer veritabanı indeksleri (height -> hash) bozulmuşsa, ham blok verisinden indeksleri yeniden inşa eder.
- **`--metrics-port`**: Düğümün sağlığını (peer sayısı, blok yüksekliği, mempool doluluğu) Prometheus formatında dışarı sunar.
- **Mainnet Bootnode Guard**: Mainnet, gerçek bootnode verilmeden başlamaz. Operatör `config/mainnet.toml` içindeki `[bootnodes].addresses` listesini doldurmalı veya `--bootstrap` geçmelidir.
- **Protocol Version Check**: Handshake ve HandshakeAck mesajlarında `version_major/version_minor` uyumluluğu kontrol edilir. Yanlış chain ID veya uyumsuz protokol gönderen peer banlanır.

---

**Tasarım Notu:**
Burada blok geldiğinde `process_incoming_block` çağrılır. Bu fonksiyon, Bölüm 3'teki `validate_block` fonksiyonunu çağırır. Eğer blok geçerliyse zincire ekler, değilse göndereni banlar (`PeerManager`). Aynı şekilde `QcBlobResponse` geldiğinde mesaj sadece loglanmaz; parse edilir, `ChainActor` üzerinden doğrulatılır ve ancak başarılıysa peer iyi davranış puanı alır.
