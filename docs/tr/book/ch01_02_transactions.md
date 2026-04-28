# Bölüm 1.2: İşlemler ve Veri Transferi Mimarisi

Bu bölüm, blok zincirindeki değer transferinin (Value Transfer) nasıl gerçekleştiğini, `Transaction` yapısının her bir parçasını neden koyduğumuzu ve `Mainnet Hardening` (Ana Ağ Güvenliği) paketleriyle gelen katı doğrulama kurallarını anlatır.

Kaynak Dosya: `src/core/transaction.rs`

---

## 1. Veri Yapıları: Bir İşlemin Anatomisi

Bir işlem (`Transaction`), "A kişisi B kişisine X miktar para gönderdi" cümlesinin dijital ve kriptografik halidir.

### Enum: `TransactionType`

İşlemler sadece para göndermek için değildir. Sistemin yönetimi için de kullanılırlar.

**Kod:**
```rust
pub enum TransactionType {
    Transfer, // Standart: Alice -> Bob (10 Coin)
    Stake,    // Validatör Olma: Paramı kilitliyorum ve ağa hizmet edeceğim.
    Unstake,  // Çıkış: Paramı çöz ve faiziyle geri ver.
    Vote,     // Yönetişim: Ağın parametrelerini değiştirmek için oy veriyorum.
    ContractCall, // BudZKVM bytecode çalıştırma.
}
```

**Neden Var?**
Eğer bu Tipler olmasaydı, Stake etmek için "Burn Adresi"ne para yollamak gibi dolambaçlı yollar (workaround) kullanmak zorunda kalırdık. İşlem tipini açıkça (`explicit`) belirtmek, kodun okunabilirliğini ve güvenliğini artırır. `AccountState` ve `Executor` bu tipe bakarak ne yapacağına karar verir.

`ContractCall`, L1 içine entegre edilen BudZKVM yürütme yoludur. Bu işlem tipinde `data` alanı memo değildir; little-endian `u64` instruction'lardan oluşan BudZKVM bytecode'dur. Executor bu bytecode'u VM'de çalıştırır, proof üretip doğrular ve ancak başarıdan sonra sender nonce/fee state'ini değiştirir.

---

### Struct: `Transaction`

**Kod:**
```rust
pub struct Transaction {
    pub from: Address,      // Gönderen (32-byte binary)
    pub to: Address,        // Alıcı (32-byte binary)
    pub amount: u64,        // Miktar
    pub fee: u64,           // İşlem Ücreti (Gas Fee)
    pub nonce: u64,         // Sıra Numarası (Anti-Replay)
    pub data: Vec<u8>,      // Ek Veri (Memo / Smart Contract Call)
    pub timestamp: u128,    // Zaman damgası
    pub hash: String,       // İşlem ID (Hex String)
    pub signature: Option<Vec<u8>>, // Dijital İmza (Zorunlu)
    pub chain_id: u64,      // Ağ ID (Chain Isolation)
    pub tx_type: TransactionType, // Tip
}
```

**Satır Satır Analiz:**

| Alan Adı | Veri Tipi | Neden Bu Tipi Seçtik? | Ne İşe Yarar & Neden Gerekli? |
| :--- | :--- | :--- | :--- |
| `from` | `Address` | 32-byte binary. | **Gönderen.** Kimin bakiyesinden düşülecek? Aynı zamanda imza doğrulamasında kullanılan Public Key'dir. |
| `to` | `Address` | 32-byte binary. | **Alıcı.** Para kime gidecek? Stake işlemlerinde boş olabilir (kendine stake). |
| `amount` | `u64` | `u64` | **Miktar.** Transfer edilecek değer. Kuruş (decimal) sorunlarıyla uğraşmamak için genellikle en küçük birim (Raw/Wei gibi) cinsinden tutulur. |
| `fee` | `u64` | `u64` | **Rüşvet / Ücret.** Madencilerin/Validatörlerin bu işlemi bloğa koyması için ödenen teşviktir. Aynı zamanda Spam saldırılarını (milyonlarca bedava işlem) engeller. |
| `nonce` | `u64` | Sıralı sayı. | **Anti-Replay Sayacı.** EN KRİTİK ALANLARDAN BİRİ. Alice Bob'a 10 coin yolladı. Bob bu işlemi ağa tekrar tekrar "replay" edip Alice'i soymasın diye var. Bir nonce sadece **BİR KERE** kullanılır. |
| `data` | `Vec<u8>`| Byte dizisi. | **Memo / Contract Payload.** Normal işlemlerde ek veri olabilir. `ContractCall` için non-empty BudZKVM bytecode'dur ve uzunluğu 8'in katı olmalıdır. |
| `timestamp`| `u128` | Epoch Zamanı. | **Zaman Penceresi.** İşlemin ne zaman üretildiği. Gelecekten (maksimum +15sn) veya çok geçmişten (maksimum -2 saat) gelen işlemler reddedilir. |
| `signature`| `Option<Vec>` | Opsiyonel Byte dizisi. | **İmza.** "Bu işlemi gerçekten `from` adresinin sahibi mi yaptı?" sorusunun cevabı. Özel anahtar (Private Key) ile atılır. *Dipnot: İşlemler bir bloğa eklenmeden önce mutlakar doğrulanır.* |
| `chain_id`| `u64` | Sabit Sayı. | **Zincir İzolasyonu.** Budlum Mainnet için üretilen bir imzalı işlemin, Budlum Testnet'te geçerli olmasını (veya tam tersi) engeller. |

---

## 2. Dinamik Ücret Piyasası (Dynamic Fee Market)

Budlum Hardening aşamasında, sabit ücret yerine **EIP-1559 benzeri** dinamik bir `base_fee` mekanizması getirilmiştir.

### Mekanizma:
- **Base Fee:** Her blok için geçerli olan minimum ücrettir.
- **Dinamik Ayar:** Her bloktan sonra ağdaki yoğunluğa (işlem sayısına) göre `base_fee` otomatik güncellenir.
  - Eğer blokta 50'den (Hedef) fazla işlem varsa: `base_fee` bir sonraki blok için **%12.5 artar**.
  - Eğer blokta 50'den az işlem varsa: `base_fee` bir sonraki blok için **%12.5 azalır** (minimum 1).
- **Spam Koruması:** Ağ saldırı altındayken ücretler hızla yükselerek saldırganın maliyetini katlar.

### 2.1 Gas Schedule ve ContractCall

`GasSchedule` artık `contract_call_gas` alanı içerir. Bu değer, `bud_estimateGas` ve ağ profillerindeki maliyet modelinin contract execution için ayrı bir taban maliyet taşımasını sağlar.

Execution tarafında BudZKVM için ayrıca sabit bir VM gas limiti uygulanır (`DEFAULT_CONTRACT_GAS_LIMIT`). Bu limit sonsuz döngü gibi DoS risklerini keser. VM out-of-gas olursa işlem başarısız sayılır ve sender state'i atomik olarak değişmeden kalır.

---

## 3. İşlem İndeksleme (TX Indexing)

Geçmiş bir işlemin hangi blokta olduğunu bulmak eskiden tüm zinciri taramayı gerektirirdi ($O(N)$).

**Yeni Mimari:**
- Her blok yazıldığında, içindeki işlemler `TX_IDX:{hash}` anahtarıyla veritabanına kaydedilir.
- Değer olarak işlemin bulunduğu **Blok Numarası** saklanır.
- Bu sayede `get_transaction_by_hash` ve `get_transaction_receipt` RPC çağrıları **O(1) (milisaniyeler içinde)** yanıt döner.

---

## 4. Algoritmalar: Güvenlik Nasıl Sağlanır?

### 2.1 Çekirdek Kriptografi: Katı İmza Doğrulaması

İşlemler (Transaction), sisteme kabul edilmeden (Mempool'a girmeden VEYA başkasının yolladığı bir bloğun içindeyse bile) %100 oranında imza kontrolünden geçer. Bu bypass edilemez. `is_valid()` fonksiyonu tüm süreçlerin kilit taşıdır.

### 2.2 Genesis Sahteciliği (Spoofing) Koruması

**Kritik Güvenlik Kuralı:** Ağdaki herhangi bir peer (düğüm), "from" adresi "genesis" olan sahte bir işlem yaratıp cüzdanları sınırsız bakiyeyle şişirmeye çalışabilir (Genesis işlemleri imzasızdır). Bu duruma karşı **Mainnet Hardening** işlemi ile şu kurallar getirilmiştir:
- İşlem Mempool'a eklenmek isteniyorsa `from == "genesis"` olanlar **ANINDA REDDEDİLİR**.
- Ağdan p2p Blok geldiyse ve eğer bu blok 0. blok (sıfırıncı - gerçek Genesis) değilse, içindeki herhangi bir işlem "genesis" olduğunu iddia ediyorsa **BÜTÜN BLOK REDDEDİLİR**.

### 2.3 Blok İçi Chain ID Doğrulaması

Bloklar ağdan alındığında, `blockchain.rs` içindeki `validate_and_add_block` fonksiyonu her bir işlemi tek tek inceler.
- Bloktaki herhangi bir işlemin `chain_id` değeri bloğun `chain_id` değerinden farklıysa (`tx.chain_id != block.chain_id`), **BLOK REDDEDİLİR**. Bu, testnet işlemlerinin mainnet bloğunun içine gömülerek validasyonun atlatılmasını engeller.

### 2.4 ContractCall Bytecode Doğrulaması

BudZKVM contract işlemleri için ek kurallar vardır:
- `amount == 0` olmalıdır. Contract execution şu MVP aşamasında native value transfer yapmaz.
- `data` boş olamaz.
- `data.len() % 8 == 0` olmalıdır. Her instruction `bud-isa::Instruction::encode()` ile üretilmiş bir little-endian `u64` olarak taşınır.
- İmza, nonce, chain ID ve fee kuralları normal işlemlerle aynıdır.

Bu kontroller `Transaction::is_valid`, `AccountState::validate_transaction_with_context` ve `Blockchain::tx_precheck` seviyelerinde görünür hale getirilmiştir. Böylece hatalı bytecode mempool'a girmeden reddedilir.

---

### Fonksiyon: `signing_hash` (İmzalanacak Veri)

Bir evrağı imzalamadan önce, neyi imzaladığınızı sabitlemeniz gerekir. Bu fonksiyon, işlemin "özünü" çıkarır.

```rust
pub fn signing_hash(&self) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    // 1. Domain Separation Tag: Karışıklığı önle.
    hasher.update(b"BDLM_TX_V2"); 
    
    // 2. Kritik alanları ekle (Binary Optimization).
    hasher.update(self.from.as_bytes());
    hasher.update(self.to.as_bytes());
    hasher.update(self.amount.to_le_bytes());
    hasher.update(self.fee.to_le_bytes());
    hasher.update(self.nonce.to_le_bytes());
    hasher.update(&self.data);
    hasher.update(self.timestamp.to_le_bytes());
    hasher.update(self.chain_id.to_le_bytes());
    
    // 3. İmzayı EKLEME!
    // 4. İşlem tipi byte'ını ekle (ContractCall = 4 dahil).
    hasher.finalize().into()
}
```

**Soru:** `hash` ve `signature` alanlarını neden eklemedik?
**Cevap:**
-   `signature`: İmzayı hesaplamak için bu hash lazım. Hash'in içine imzayı koyamazsınız (Tavuk-Yumurta problemi).
-   `hash`: İşlemin ID'si (TxID) genellikle tüm verinin (imza dahil) hash'idir. İmzalamadan önce ID belli olmayabilir.

**Tasarım Notu:** `nonce` ve `chain_id` alanlarını bu hash'e dahil etmek ZORUNLUDUR. Yoksa Replay Attack (Tekrar Saldırısı) yapılabilir.
-   Chain ID dahil olmasaydı: Testnet'teki işlem Mainnet'te de geçerli olurdu.
-   Nonce dahil olmasaydı: Geçmişteki bir para transferi tekrar tekrar gönderilip bakiye boşaltılırdı.

---

### Fonksiyon: `sign` (İmzalama Süreci)

Bu fonksiyon client (cüzdan) tarafında çalışır.

```rust
pub fn sign(&mut self, keypair: &KeyPair) {
    // 1. Güvenlik Kontrolü: Yanlış anahtarla mı imzalamaya çalışıyoruz?
    // İşlemdeki 'from' adresi, elimizdeki Private Key'e ait mi?
    if self.from != keypair.public_key_hex() {
        println!("HATA: Başkasının adına imza atamazsın!");
    }

    // 2. İmzalanacak özeti çıkar.
    let hash = self.signing_hash();
    
    // 3. Kriptografik imza üret (Ed25519).
    let signature = keypair.sign(&hash);
    
    // 4. İmzayı işlem nesnesine yapıştır.
    self.signature = Some(signature);
}
```

**Benzerlik:** Islak imza atmak gibidir. Önce metni yazarsınız (`signing_hash`), sonra altına imza atarsınız (`sign`). Metin değişirse, bir bit dahi oynarsa imza anında matematiken geçersiz kalır.
