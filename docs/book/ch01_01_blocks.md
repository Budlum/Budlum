# Bölüm 1.1: Blok Yapısı ve Zincir Mimarisi

Bu bölüm, blok zincirinin en temel yapı taşı olan **BLOK** kavramını, kaynak koddaki her satırın neden yazıldığını açıklayarak öğretir.

Kaynak Dosya: `src/core/block.rs`

---

## 1. Veri Yapıları: Neyi, Neden Tutuyoruz?

Blok zincirinde veriyi rastgele tutamayız. Her baytın bir amacı ve maliyeti vardır. Budlum projesinde blok yapısını ikiye ayırdık: `BlockHeader` ve `Block`.

### Struct: `BlockHeader` (Blok Başlığı)

Bu yapı, bir bloğun **kimliğidir**. İçinde işlem verisi (transaction data) bulunmaz, sadece özetler bulunur.

**Kod:**
```rust
pub struct BlockHeader {
    pub index: u64,
    pub timestamp: u128,
    pub previous_hash: String,
    pub hash: String,
    pub producer: Option<String>,
    pub chain_id: u64,
    pub state_root: String,
    pub tx_root: String,
    pub slashing_evidence: Option<Vec<SlashingEvidence>>,
    pub nonce: u64,
    // --- Hardening Phase 2: VRF & Finalite ---
    pub epoch: u64,
    pub slot: u64,
    pub proposer_pubkey: Option<String>,
    pub vrf_output: Vec<u8>,
    pub vrf_proof: Vec<u8>,
    pub validator_set_hash: String,
}
```

**Satır Satır Analiz:**

| Alan Adı | Veri Tipi | Neden Bu Tipi Seçtik? | Ne İşe Yarar & Neden Gerekli? |
| :--- | :--- | :--- | :--- |
| `index` | `u64` | `u32` (4 milyar) yetersiz kalabilir. `u64` sonsuza kadar yeter. | **Sıra Numarası.** Bloğun zincirdeki konumunu belirtir. Genesis bloğu 0'dır. Zincirin uzunluğunu ölçmek için kritiktir. |
| `timestamp` | `u128` | Milisaniye hassasiyeti için `u64` bazen taşabilir (yüzyıllar sonra). `u128` garantidir. | **Zaman Damgası.** Bloğun ne zaman üretildiğini kanıtlar. Zorluk ayarlaması (PoW) ve Epoch geçişleri (PoS) bu süreye göre yapılır. |
| `previous_hash`| `String` | 64 karakterlik Hex String (Okunabilir olması için). | **Zincir Bağlantısı.** Önceki bloğun parmak izidir. Bu alan sayesinde bloklar birbirine "zincirlenir". Eğer önceki blokta 1 bit değişirse, onun hash'i değişir ve bu bağlantı kopar. Güvenliğin temelidir. |
| `hash` | `String` | Hex String. | **Bloğun Kimliği.** Bu başlığın (header) SHA3-256 özetidir. Bloğu tanımlamak için kullanılır. |
| `producer` | `Option<String>`| Opsiyonel (`Option`), çünkü blok hazırlanırken henüz üreticisi belli olmayabilir. | **Blok Üreticisi.** Bloğu bulan madencinin veya validatörün Halka Açık Anahtarı (Public Key). Ödülü kimin alacağını belirler. |
| `chain_id` | `u64` | Sayısal ID (Budlum için 1337). | **Ağ Kimliği.** Testnet (deneme ağı) ile Mainnet (ana ağ) bloklarının karışmasını önler. Replay saldırılarına karşı izolasyon sağlar. |
| `state_root` | `String` | 32-byte Hex Hash. | **Hesap Durumu Özeti.** O andaki tüm hesap bakiyelerinin Merkle köküdür. Hafif istemciler (telefondaki cüzdanlar), tüm veritabanını indirmeden "X'in bakiyesi Y'dir" bilgisini bununla doğrular. |
| `tx_root` | `String` | 32-byte Hex Hash. | **İşlem Özeti.** Bloktaki tüm işlemlerin Merkle köküdür. Blok içindeki işlemlerin değiştirilemezliğini sağlar. |
| `slashing_evidence` | `Option<Vec>` | Opsiyonel Liste. Her blokta ceza olmak zorunda değil. | **Suç Kanıtları (PoS).** Kötü niyetli validatörlerin (çift imza atanların) kanıtlarını taşır. Bu kanıtlar blokta yer alırsa, o validatörler cezalandırılır. |
| `nonce` | `u64` | Dönüştürelebilir sayı. | **İş Kanıtı Sayacı (PoW).** Madencilerin hedef hash'i tutturmak için sürekli değiştirdiği deneme sayısıdır. PoS modunda 0 olabilir. |
| `epoch` | `u64` | Periyodik döngü sayısı. | **Dönem Bilgisi.** Validatör setinin ve lider seçim tohumunun (seed) güncellendiği zaman dilimidir. |
| `slot` | `u64` | Zaman dilimi indeksi. | **Slot Numarası.** Belirli bir epoch içindeki zaman dilimi. Her slotta bir lider blok üretme hakkına sahiptir. |
| `vrf_output` | `Vec<u8>` | Kriptografik rastgele çıktı. | **Piyango Çıktısı.** Liderin seçildiğini kanıtlayan deterministik ancak tahmin edilemez rastgele değer. |
| `vrf_proof` | `Vec<u8>` | VRF kanıtı. | **Doğrulama Kanıtı.** Diğer düğümlerin, VRF çıktısının doğru üretildiğini kontrol etmesini sağlar. |
| `validator_set_hash`| `String` | 32-byte Hex Hash. | **Validatör Seti Özeti.** Bloğun üretildiği andaki aktif validatörlerin özetidir. Finalite katmanı için kritik bir referanstır. |

---

### Struct: `Block` (Tam Blok)

Bu yapı, başlığın yanı sıra asıl veriyi (işlemleri) taşır.

**Kod:**
```rust
pub struct Block {
    // ... Header alanlarının aynısı (index, timestamp, vb.) ...
    pub transactions: Vec<Transaction>,
    pub signature: Option<Vec<u8>>,
}
```

**Analiz:**

| Alan Adı | Veri Tipi | Ne İşe Yarar? |
| :--- | :--- | :--- |
| `transactions` | `Vec<Transaction>` | **İşlem Listesi.** Para transferleri, kontrat çağrıları vb. Bloğun "yükü" (payload) burasıdır. Diskte yer kaplayan asıl kısım budur. |
| `signature` | `Option<Vec<u8>>` | **Üretici İmzası.** `producer` alanındaki kişinin bu bloğu gerçekten onayladığını kanıtlayan Ed25519 imzası. Olmazsa, herkes başkasının adına blok üretebilirdi. |

---

## 2. Algoritmalar ve Fonksiyonlar: Nasıl Çalışır?

### Fonksiyon: `calculate_hash`

Bir bloğun "parmak izini" (Hash) oluşturur.

```rust
pub fn calculate_hash(&self) -> String {
    // 1. Önce opsiyonel alanları byte dizisine çevir (Serialization)
    let producer_bytes = self.producer.as_ref().map(...).unwrap_or_default();
    
    // 2. slashing_evidence için bincode kullanarak deterministik serileştirme yap
    let evidence_bytes = self.slashing_evidence.as_ref().map(|e| bincode::serialize(e).unwrap_or_default()).unwrap_or_default();
    
    // 3. hash_fields fonksiyonuna tüm verileri sırayla besle
    hash_fields(&[
        b"BDLM_BLOCK_V2",              // <--- Domain Separation Tag
        &self.index.to_le_bytes(),     // Sayıları byte'a çevir (Little Endian)
        &self.timestamp.to_le_bytes(),
        self.previous_hash.as_bytes(),
        self.tx_root.as_bytes(),
        &self.nonce.to_le_bytes(),
        // VRF & Epoch Alanları (Deterministik Hash için)
        &self.epoch.to_le_bytes(),
        &self.slot.to_le_bytes(),
        &self.vrf_output,
        self.validator_set_hash.as_bytes(),
    ])
}
```

**Neden Böyle Yazdık?**
1.  **Domain Separation (`b"BDLM_BLOCK_V2"`):** Bu sabit metin (magic bytes), farklı veri tiplerinin (Transaction ve Block) hashlerinin karışmasını engeller. Eğer bir işlem verisi tesadüfen bir blok verisine benzerse, hashleri aynı çıkmaz çünkü blok hashlerken başına bu etiketi ekliyoruz. Bu profesyonel bir güvenlik standardıdır.
2.  **Deterministik Serileştirme (`bincode`):** Özellikle `slashing_evidence` gibi karmaşık liste yapılarını baytlara çevirirken, `serde_json` gibi veri sıralamasını değiştirebilecek formatlar yerine doğrudan makine dostu `bincode` kullanılarak network split (hash uyuşmazlığı) sorunları engellenir.
3.  **Little Endian (`to_le_bytes`):** Farklı işlemci mimarilerinde (Intel vs ARM) sayıların bellekte tutulma sırası farklıdır. Ağda herkesin aynı hash'i bulması için sayıları standart bir formata (Little Endian) zorlarız.
4.  **Tüm Alanlar:** Hash'e *her şeyi* dahil ederiz (nonce, timestamp, rootlar). Böylece bloktaki en ufak bir virgül değişse, hash tamamen değişir (Avalanche Effect).

---

### Fonksiyon: `calculate_tx_root` (Merkle Root Hesabı)

Bu fonksiyon, bloktaki binlerce işlemin tek bir hash (32 byte) ile temsil edilmesini sağlar.

```rust
pub fn calculate_tx_root(&self) -> String {
    // 1. Tüm işlemlerin hash'lerini alıp bir listeye koy.
    let mut tx_hashes: Vec<String> = self.transactions.iter().map(|tx| tx.hash.clone()).collect();

    // 2. Eğer hiç işlem yoksa, boş hash dön (Genesis durumu).
    if tx_hashes.is_empty() { return "0".repeat(64); }

    // 3. Tek bir kök kalana kadar döngüye gir.
    while tx_hashes.len() > 1 {
        let mut next_level = Vec::new();
        // 4. Listeyi ikişer ikişer (chunk) gez.
        for chunk in tx_hashes.chunks(2) {
            let left = &chunk[0];
            // 5. Eğer sayı tekse, son elemanı kopyala (A, B, C -> A+B, C+C)
            let right = if chunk.len() > 1 { &chunk[1] } else { left };
            
            // 6. İkisini birleştirip hashle: Hash(Left + Right)
            let combined = format!("{}{}", left, right);
            next_level.push(hex::encode(hash(combined)));
        }
        // 7. Bir üst seviyeye geç.
        tx_hashes = next_level;
    }
    // 8. Kalan son hash, Merkle Köküdür.
    tx_hashes[0].clone()
}
```

**Neden Bunu Yaptık?**
-   **Verimlilik:** Bir bloğun içinde "Ahmet'in Mehmet'e 5 coin attığı" işlemin var olduğunu kanıtlamak için, tüm bloğu indirmeye gerek kalmaz. Sadece Merkle yolunu (Path) indirerek kök hash ile eşleştiğini doğrularız.
-   **Light Client Desteği:** Cep telefonları bu sayede güvenli işlem yapabilir.

---

### Fonksiyon: `verify_signature` (İmza Doğrulama)

Bir bloğun gerçekten iddia edilen kişi tarafından üretildiğini doğrular.

```rust
pub fn verify_signature(&self) -> bool {
    // 1. Producer (Üretici) var mı? Yoksa geçersiz.
    let producer_hex = match &self.producer { ... };
    
    // 2. İmzası var mı? Yoksa geçersiz.
    let signature = match &self.signature { ... };
    
    // 3. HEX formatındaki anahtarı byte dizisine çevir.
    let public_key = hex::decode(producer_hex)...;

    // 4. KRİTİK ADIM: Bloğun hash'ini elindeki verilerle YENİDEN HESAPLA.
    // Asla blok üzerinde yazan 'self.hash'e güvenme, hileli olabilir.
    let calculated_hash = self.calculate_hash();
    if calculated_hash != self.hash { return false; } // Veri bütünlüğü bozuk!

    // 5. Kriptografik doğrulama yap (Ed25519)
    verify_signature(self.hash.as_bytes(), signature, &public_key).is_ok()
}
```

**Neden Kritiktir?**
Blok zincirinde "Ben ürettim" demek yetmez, bunu matematiksel olarak ispatlamak gerekir. Bu fonksiyon, bloğun içeriğinin (transactions, timestamp vb.) değiştirilmediğini ve gerçekten o açık anahtarın sahibi tarafından imzalandığını garanti eder.

---

### Fonksiyon: `mine` (Madencilik - PoW)

İş Kanıtı (Proof of Work) algoritmasının kalbidir.

```rust
pub fn mine(&mut self, difficulty: usize) {
    // 1. Hedef belirle: Başında 'difficulty' kadar sıfır olan bir string.
    // Örn: difficulty=2 ise target="00"
    let target = "0".repeat(difficulty);

    // 2. Sonsuz döngüye gir.
    while !self.hash.starts_with(&target) {
        // 3. Nonce (deneme sayacı) değerini artır.
        self.nonce += 1;
        
        // 4. Hash'i yeniden hesapla. Nonce değiştiği için hash tamamen değişecektir.
        self.hash = self.calculate_hash();
    }
    // 5. Döngü biterse, hedefi tutturduk demektir. Blok hazır!
}
```

**Neden Yapıyoruz?**
Bu işlem bilgisayarı yorar (CPU/GPU gücü harcar). Bir saldırganın geçmişe dönük bir bloğu değiştirmesi için, o bloktan sonra gelen **tüm bloklar için** bu yorucu işlemi tekrar yapması gerekir. Bu da saldırıyı ekonomik olarak imkansız hale getirir. Buna **Immutability (Değiştirilemezlik)** denir.
