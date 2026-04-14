# Bölüm 5.3: Snapshot ve Veri Budama (Pruning)

Bu bölüm, blok zincirinin sonsuza kadar büyümesini engelleyen **Pruning (Budama)** mekanizmasını ve ağa yeni katılanların günlerce beklemeden senkronize olmasını sağlayan **Snapshot (Anlık Görüntü)** stratejisini analiz eder.

Kaynak Dosya: `src/chain/snapshot.rs`

---

## 1. Problem: Zincir Şişkinliği (State Bloat)

Bir blok zinciri 10 yıl çalışırsa ne olur?
-   **Bitcoin:** 500 GB+ veri.
-   **Ethereum:** 1 TB+ veri (Arşiv Node).

Her düğümün bu kadar veriyi saklaması pahalıdır. Disk dolar, indeksleme yavaşlar.
Çözüm: **Sadece son durumu sakla, geçmişi sil.**

---

## 2. Veri Yapıları: Anlık Görüntü

Snapshot, bir video kaydının (Blockchain) tek bir karesini (State) alıp JPG olarak saklamak gibidir.

### Struct: `Snapshot`

```rust
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub height: u64,             // Hangi blokta çekildi? (Örn: 100.000)
    pub state_root: String,      // O anki hesap durumunun özeti
    pub accounts: Vec<Account>,  // Tüm hesap bakiyeleri
    pub validators: Vec<Validator>, // O anki validatör listesi
    // --- Hardening Phase 2: Finalite Farkındalığı ---
    pub finalized_height: u64,   // Kesinleşmiş son yükseklik
    pub finalized_hash: String,  // Kesinleşmiş son bloğun hash'i
}
```

**Analiz:**
Bu yapı, `AccountState`'in serileştirilmiş (paketlemiş) halidir. 

**Hardening İyileştirmesi:**
- **Validator Set:** Snapshot artık tüm aktif validatör setini ve stake miktarlarını içerir. Bu sayede Fast Sync ile ağa yeni katılan bir node, checkpoint imzalarını doğrulamak için genesis'ten beri gelen tüm validatör değişimlerini izlemek (replay) zorunda kalmaz.
- **Sync Sürekliliği:** Snapshot uygulandığında, eksik kalan blok yükseklikleri için "stub block"lar oluşturularak veritabanı indeksleme bütünlüğü korunur.
- **Finality Awareness:** Snapshot, finalized checkpoint bilgisini de taşıdığı için budama ve recovery sırasında kesinleşmiş kısmın altına inilmez.

---

## 3. Algoritmalar: Budama Mantığı

### Fonksiyon: `create_snapshot` (Fotoğraf Çekme)

Belirli aralıklarla (örneğin her 10.000 blokta bir / Epoch sonu) çalışır.

```rust
pub fn create_snapshot(&self, state: &AccountState) {
    if state.epoch_index % SNAPSHOT_INTERVAL == 0 {
        let snapshot = Snapshot {
            height: state.epoch_index,
            accounts: state.accounts.values().cloned().collect(),
            // ...
        };
        
        // Diske kaydet: "snapshot_100000.bin"
        save_to_disk(snapshot);
    }
}
```

### Fonksiyon: `prune_history` ve Otokontrol (Pruning Hook)

Snapshot alınırken, veritabanını (`sled::Db`) yöneten `Storage` modülüne eski verileri silmesi emredilir.

Budlum Core'daki ana blok işleme döngüsü (bkz: `Blockchain::validate_and_add_block`) her yeni blok geldiğinde şunu sorar:

```rust
if let Some(ref pruning_manager) = self.pruning_manager {
    let height = last_block.index;
    
    // 1. Snapshot alma vakti geldi mi? (Örn: Her 10.000 blokta bir)
    if pruning_manager.should_create_snapshot(height) {
        
        let snapshot = StateSnapshot::from_state(height, last_block.hash.clone(), self.chain_id, &self.state);
        pruning_manager.save_snapshot(&snapshot);
        
        // 2. Güvenlik marjı dışında kalan, artık ihtiyacımız olmayan eski blokları bul.
        let prunable = pruning_manager.get_prunable_blocks(self.chain.len() as u64, height);
        
        if !prunable.is_empty() {
            if let Some(ref store) = self.storage {
                // 3. Blokları ve State eşlemelerini Hard Diskten tamamen sil (Pruning).
                // DİKKAT: PruningManager, finalized_height'ın altındaki blokların 
                // asla silinmemesini garanti eder.
                for block_index in &prunable {
                    let _ = store.delete_block(*block_index);
                }
            }
        }
    }
}
```

**Neden Finalite Farkındalığı?**
Eskiden sadece `min_blocks` (güvenlik tamponu) kullanılırken, artık **Finalized Checkpoint** asıl referans noktasıdır. Hiçbir budama işlemi, ağın %100 kesinlik verdiği (finalized) bir bloğu silecek kadar geri gidemez. Bu, veri bütünlüğü için en üst düzey sigortadır.

## 3.5 Replay Semantiği

Snapshot veya diskten zincir yüklenirken state yeniden inşası yalnızca "işlemleri sırayla uygula" şeklinde yapılmaz. Budlum'un güncel akışında:

- blok ödülü,
- slashing etkileri,
- epoch geçişleri,
- dinamik base fee güncellemeleri

aynı zincir kabul yolunda olduğu gibi replay sırasında da tekrar uygulanır.

Bu sayede node yeniden başlatıldığında veya reorg sonrası state yeniden kurulduğunda, çalışma anındaki state ile recovery sonrası state aynı kalır.

---

**Neden Güvenlik Marjı (Safety Margin)?**
Zincirin en ucunda bazen çatallanmalar (Micro-forks) olur. Son 10-20 blok değişebilir. Eğer snapshot alır almaz hemen önceki blokları silersek ve zincir başka bir dala (Reorg) geçerse, verisiz kalırız ve düğüm çöker. Bu yüzden `PruningManager::new(min_blocks, ...)` ayarlandığında, her zaman bir miktar "tampon bölge" (buffer) bırakılır.

---

## 4. State Sync (Hızlı Senkronizasyon)

Yeni bir düğüm kurduğunuzu düşünün.
-   **Full Sync (Yavaş):** Genesis'ten başla. 1 Milyon bloğu tek tek indir, işlemleri çalıştır (`apply_transaction`). Aylar sürebilir.
-   **State Sync (Hızlı):**
    1.  Arkadaşına sor: "En son snapshot kaç?" -> "1.000.000"
    2.  Snapshot dosyasını indir (500 MB).
    3.  `AccountState`'i bu dosyadan yükle.
    4.  Sadece 1.000.001'den itibaren blokları indirmeye başla.
    5.  Süre: 10 Dakika.

Kullanıcılarımız için **State Sync** varsayılan yöntem olmalıdır.

---

## Özet

`src/snapshot.rs` ve Budama mekanizması sayesinde:
1.  **Disk Tasarrufu:** TB'larca gereksiz veri saklanmaz.
2.  **Hız:** Yeni düğümler ağa dakikalar içinde katılır.
3.  **Sürdürülebilirlik:** Blok zinciri sonsuza kadar çalışabilir.
