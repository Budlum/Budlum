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
    pub swarm: Swarm<BudlumBehaviour>, // Ağ Motoru
    pub blockchain: Arc<Mutex<Blockchain>>, // Zincir Verisi (Paylaşımlı)
    pub peer_manager: Arc<Mutex<PeerManager>>, // Eş Yönetimi
    pub peer_count: Arc<AtomicUsize>, // Bağlı Eş Sayısı (Gerçek Zamanlı)
    command_rx: mpsc::Receiver<NodeCommand>, // İçerden gelen emirler
    // ...
}
```

### Struct: `NodeClient`

Dış modüllerin (örn: RPC Sunucusu) Düğüm ile güvenli bir şekilde konuşmasını sağlayan hafif bir "kumanda" yapısıdır.

```rust
pub struct NodeClient {
    sender: mpsc::Sender<NodeCommand>,
    pub peer_id: PeerId,
    pub peer_count: Arc<AtomicUsize>,
}
```

**Tasarım Kararı: `Arc<Mutex<Blockchain>>`**
-   `Arc` (Atomic Reference Counting): Blockchain verisi RAM'de tek bir yerde durur, ama hem `Node` hem `Miner` hem `API` ona erişebilir. Veri kopyalanmaz, referans paylaşılır.
-   `Mutex` (Mutual Exclusion): Aynı anda sadece bir kişi yazabilir. Veri bütünlüğünü (Data Race) engeller.

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
        }
    }
}
```

**Analiz: `tokio::select!`**
Bu makro, Go dilindeki `select` gibidir. Birden fazla asenkron işlemden hangisi **önce** gerçekleşirse onu çalıştırır.
-   Eğer ağdan veri geldiyse, onu işler.
-   Eğer ağ sessizse ama 60 saniye dolduysa, çöp toplayıcı (GC) görevlerini tetikler.
-   Eğer ağ sessizse ama kullanıcı "Blok üret" dediyse, onu işler.
-   Hiçbir şey yoksa, işlemciyi uyutur (Idle). Enerji tasarrufu sağlar.

---

### Fonksiyon: `handle_network_event`

Ağdan gelen paketleri açtığımız yer.

```rust
async fn handle_network_event(&mut self, event: SwarmEvent<BudlumBehaviourEvent>) {
    match event {
        // Yeni bir Blok veya İşlem geldiğinde (Gossipsub)
        SwarmEvent::Behaviour(BudlumBehaviourEvent::Gossipsub(gossip_event)) => {
            if let GossipsubEvent::Message { message, .. } = gossip_event {
                // Mesajı ayrıştır (Deserialize)
                let network_msg: NetworkMessage = bincode::deserialize(&message.data).unwrap();
                
                match network_msg {
                    NetworkMessage::Block(block) => {
                        println!("📦 Yeni blok geldi: #{}", block.index);
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

**Analiz: Peer Count Takibi**
`run` döngüsü içinde `SwarmEvent::ConnectionEstablished` olduğunda `peer_count` artırılır, `ConnectionClosed` olduğunda azaltılır. Bu veri atomik olduğu için RPC sunucusu tarafından kilitlenme (lock) gerektirmeden anlık okunabilir.
        
        // Yeni biri bağlandığında (Connection Established)
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            println!("🤝 Yeni arkadaş: {}", peer_id);
            // Onu tanımak için Kademlia'ya ekle
            self.swarm.behaviour_mut().kad.add_address(&peer_id, ...);
        }
        
        // ...
    }
}
```

**Tasarım Notu:**
Burada blok geldiğinde `process_incoming_block` çağrılır. Bu fonksiyon, Bölüm 3'teki `validate_block` fonksiyonunu çağırır. Eğer blok geçerliyse zincire ekler, değilse göndereni banlar (`PeerManager`). Hepsi birbirine bağlıdır.
