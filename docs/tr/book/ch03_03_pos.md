# Bölüm 3.3: Proof of Stake (PoS) Motoru ve RANDAO

Bu bölüm, modern blok zincirlerinin tercihi olan PoS (Hisse Kanıtı) algoritmasını; **VRF (Verifiable Random Function) tabanlı lider seçim matematiğini**, çift blok üretme (Double Proposal) ceza sistemini ve konsensüs güvenliğini satır satır inceler.

Kaynak Dosya: `src/consensus/pos.rs`

---

## 1. Veri Yapıları: Oyunun Kuralları

PoS, parası olanın söz sahibi olduğu, ancak hata yapanın parasını kaybettiği bir ekonomik oyundur.

### Struct: `PoSConfig` ve `PoSEngine`

```rust
pub struct PoSConfig {
    pub min_stake: u64,          // Min. Teminat
    pub slot_duration: u64,      // Her blok kaç ms? (SLOT_MS: 3000)
    pub epoch_length: u64,       // Bir devir kaç blok sürer?
    pub slashing_penalty_scaled: u64, // Fixed-point ceza oranı (Örn %10 = 100,000 / SCALE)
}

pub struct PoSEngine {
    config: PoSConfig,
    seen_blocks: RwLock<HashMap<(String, u64), String>>, 
    slashing_evidence: RwLock<Vec<SlashingEvidence>>,    
    epoch_seed: RwLock<[u8; 32]>,                        
    storage: Option<Storage>,                            // Kalıcılık için disk bağlantısı
    keypair: Option<KeyPair>,                            
}
```

**Analiz:**
- **`epoch_seed` (RANDAO Ortak Tohumu):** Ağdaki rastgelelik (randomness) kaynağıdır. `RwLock` ile korunur. Eski tasarımdaki tekil blok bağımlılığını (ve manipülasyonları) çözer.

---

## 2. Algoritmalar: RANDAO Lider Seçimi ve Ceza

### Fonksiyon: `expected_proposer` (VRF Lider Seçimi)

Her slot için kimin blok üreteceğini belirleyen "Kriptografik Piyango" fonksiyonudur. Eski RANDAO yapısı, **Hardening Phase 2** ile VRF tabanlı bir sisteme dönüştürülmüştür.

```rust
pub fn expected_proposer(&self, slot: u64, validators: &[Validator]) -> Option<Validator> {
    // 1. Rastgeleliği Kanıtla (VRF)
    // Lider, kendi Private Key'i ve Slot numarasını kullanarak 
    // bir VRF çıktısı (output) ve kanıtı (proof) üretir.
    
    // 2. Eşik Değeri (Threshold) Hesabı
    // Threshold = 2^256 * (Hisse / Toplam_Hisse)
    let threshold = self.calculate_vrf_threshold(validator_stake, total_stake);

    // 3. Piyango Çıkış Kontrolü
    // Eğer VRF_Output < Threshold ise, o validatör o slotun lideridir.
    if vrf_output < threshold {
        return Some(validator);
    }
    None
}
```

**Neden VRF (RANDAO'ya Karşı)?**
- **Sıfır Manipülasyon (Bias-Resistance):** RANDAO'da son blok üreticisi hash'i manipüle ederek gelecekteki liderleri etkileyebilir (bias). VRF'de ise çıktı sadece liderin gizli anahtarına bağlıdır ve deterministiktir; kimse (lider dahil) sonucu önceden değiştiremez.
- **Gizlilik:** Kimin lider olacağı, o slot gelene ve lider kanıtını sunana kadar ağ tarafından bilinmez. Bu, DoS saldırılarına karşı koruma sağlar.

### Determinizm ve Sabit Noktalı Matematik (Fixed-Point Math)

**Budlum Hardening** ile birlikte, tüm platformlarda (Mac, Windows, Linux) aynı sonucun alınması için `f64` kullanımı tamamen kaldırılmıştır.

```rust
pub fn check_vrf_threshold(&self, vrf_output: [u8; 32], stake: u64, total_stake: u64) -> bool {
    // Threshold = (MAX_U256 * stake) / total_stake
    // Tüm hesaplamalar u256/u128 üzerinden tam sayı aritmetiği ile yapılır.
    let threshold = self.calculate_threshold(stake, total_stake);
    u256_from_be_bytes(vrf_output) < threshold
}
```

### Slot-Bazlı Deterministik Zaman Damgaları

Artık blok zamanları `SystemTime::now()` ile değil, genesis zamanından itibaren geçen slot sayısına göre hesaplanır:
`timestamp = genesis_time + (block_index * SLOT_MS)`
Bu sayede ağdaki saat farkları (clock drift) fork'a sebep olamaz.

---

## 3. Slashing Kanıtları: Suç ve Ceza

### Double Proposal (Çift Blok Üretimi)

Bir liderin aynı slot içinde iki farklı blok üretip imzalamasıdır. Bu, zinciri bölme girişimi (forking) olarak kabul edilir.

- **Tespit:** `seen_blocks` tablosunda aynı `slot` ve `producer` için farklı `block_hash` yakalandığında tetiklenir.
- **Kanıt:** İki farklı bloğun başlığı ve imzaları `SlashingEvidence::double_proposal` olarak paketlenir.
- **Ceza:** Suçlu validatörün hissesinin belirli bir kısmı (örn. %10) silinir ve validatör sistemden atılır.
- **Gossip:** Kanıt sadece tespit eden node'da kalmaz. Node bunu `NetworkMessage::SlashingEvidence` olarak ağa yayar; diğer producer'lar da kanıtı bloklarına dahil edebilir.

---

### Fonksiyon: `record_block` (Kalıcılık ve Dedektiflik)

Ağa gelen her bloğu kaydeder. **Mainnet Ready** aşamasında, bu fonksiyon artık değişiklikleri anında diske (`Storage`) yazar.

```rust
pub fn record_block(&self, block: &Block) {
    // 1. Double-Sign Tespiti
    // 2. Eğer geçerliyse, konsensüs durumunu (seen_blocks, seed) diske kaydet.
    if let Some(ref storage) = self.storage {
        storage.save_consensus_state(&self.get_state()); 
    }
}
```

**Neden RANDAO (XOR-Mix)?**
Eski yapıda `previous_hash` kullanılıyordu. Bir düğüm çıkaracağı bloğu manipüle edip ufak TX değişiklikleri ile hash'i değiştirerek "sıradaki bloğu da" kendine düşürebilirdi. 
RANDAO ile, tüm blokların hash'leri ardışık olarak (`XOR` işlemi) birbirine karıştırılır. Epoch bitene kadar hiçkimse tam teşekküllü Epoch Tohumu'nun ne olacağını %100 kestiremez ve oyun oynayamaz (Bias-Resistance).

---

### Fonksiyon: `prepare_block` (Blok Üretimi)

Eğer sıra bizdeyse çalışır.

```rust
fn prepare_block(&self, block: &mut Block, state: &AccountState) {
    // 1. Önce bekleyen "Suç Kanıtları"nı bloğa ekle. Adalet gecikmemeli.
    {
        let mut evidence_pool = self.slashing_evidence.write().unwrap();
        if !evidence_pool.is_empty() {
            block.slashing_evidence = Some(evidence_pool.clone());
            evidence_pool.clear(); 
        }
    }

    // 2. İmza At.
    if let Some(keypair) = &self.keypair {
        block.sign(keypair);
    }
}
```

**Tasarım Notu:**
Ceza kanıtlarını (`slashing_evidence`) önce gossip ile ağda dolaştırıyoruz, sonra blokların içine koyuyoruz. Böylece cezayı uygulamak yalnızca suçu ilk gören node'un blok üretmesine bağlı kalmaz; herhangi bir producer geçerli kanıtı zincire taşıyabilir.

---

## Özet

`src/consensus/pos.rs`, bir yazılım kodundan ziyade bir "Anayasa" gibidir.
-   **Seçim Kanunu:** `select_validator` ve `epoch_seed` ile RANDAO rastgeleliğinde kimin yöneteceğini belirler.
-   **Ceza Kanunu:** `record_block` ve `SlashingEvidence` ile kurallara uymayanlar cezalandırılır.
-   **Yürütme:** `prepare_block`, pending evidence ve blok ödülü akışıyla kararları zincire taşır.
