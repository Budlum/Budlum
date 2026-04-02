# Bölüm 1.3: Hesap Durumu ve State Machine Mimarisi

Bu bölüm, blok zincirinin "hafızası" olan `AccountState` yapısını, validatör yönetimini ve `epoch` mantığını en ince detayına kadar açıklar.

Kaynak Dosya: `src/account.rs`

---

## 1. Veri Yapıları: Hafızada Neler Var?

Blok zinciri duran bir veri değildir, sürekli değişen bir durum makinesidir (State Machine).

### Struct: `Account` (Banka Hesabı)

**Kod:**
```rust
pub struct Account {
    pub public_key: Address, // Hesap Numarası (32-byte binary)
    pub balance: u64,       // Bakiye
    pub nonce: u64,         // İşlem Sayacı
}
```

**Satır Satır Analiz:**
-   `balance`: Neden `i64` (negatif olabilir) değil de `u64`? Çünkü bakiye asla negatif olamaz. Bu tip seçimi, kodun güvenliğini matematiksel olarak artırır (Underflow koruması).
-   `nonce`: Her giden işlemde (`outgoing tx`) bu sayı 1 artar. Ağ, gelen işlemin nonce'u ile hesaptaki nonce'u kıyaslar. Eşit değilse işlemi reddeder. Bu, **sıralı işlem garantisi** ve **replay koruması** sağlar.

---

### Struct: `Validator` (Sistem Bekçisi)

Validatörler, sistemin güvenliğini sağlayan özel hesaplardır.

**Kod:**
```rust
pub struct Validator {
    pub address: Address,   // 32-byte binary
    pub stake: u64,         // Kilitlenen Teminat
    pub active: bool,       // Şu an görevde mi?
    pub slashed: bool,      // Kırmızı kart yedi mi?
    pub jailed: bool,       // Geçici uzaklaştırma (sarı kart)
    pub jail_until: u64,    // Ne zaman dönebilir?
    pub last_proposed_block: Option<u64>, // Aktivite takibi
}
```

**Tasarım Kararları:**
-   `slashed` ve `jailed` farkı nedir?
    -   **Jailed (Hapis):** Geçici bir durumdur. Örneğin, validatör offline oldu ve blok kaçırdı. Bir süre dinlendirilir (ceza süresi dolana kadar). Sonra geri gelebilir.
    -   **Slashed (Kesme):** Kalıcı ve ağır bir durumdur. Validatör kötü niyetli bir şey yapmıştır (Double Sign). Parası silinir ve sistemden atılır.

---

### Struct: `AccountState` (Global Hafıza)

**Kod:**
```rust
    pub accounts: BTreeMap<Address, Account>,   // Tüm hesaplar (sıralı)
    pub validators: BTreeMap<Address, Validator>, // Tüm validatörler (sıralı)
    pub base_fee: u64,                          // Dinamik ağ işlem ücreti
    pub block_reward: u64,                      // Dinamik blok ödülü
    storage: Option<Storage>,                   // Disk bağlantısı
    pub epoch_index: u64,                       // Zaman dilimi sayacı
    dirty_accounts: HashSet<Address>,           // Değişen hesapların takibi
    cached_tree: Vec<Vec<[u8; 32]>>,            // Incremental Merkle Tree
}
```

**Neden Dirty Tracking?**
Blockchain'de milyonlarca hesap olabilir. Her blokta tüm hesapları baştan hashlemek sistemi yavaşlatır. `dirty_accounts` set'i sayesinde sadece o blokta değişen hesaplar işaretlenir ve Merkle Trie sadece bu dalları günceller. Bu, **Incremental State Root** hesaplamasının kalbidir.

---

**Budlum Hardening** aşamasında, `state_root` hesaplaması artık **Incremental Merkle Trie** yapısıdır.

**Kod (Özet):**
```rust
pub fn calculate_state_root(&mut self) -> String {
    // 1. Sadece dirty_accounts içindeki hesaplar için yeni yaprakları (leaves) hesapla.
    // 2. cached_tree yapısını kullanarak sadece değişen dalları güncelle ($O(\log N)$).
    // 3. Root hash'ini döndür.
    // 4. Dirty listesini temizle.
}
```

**Neden Incremental Merkle Trie?**
1. **Düşük Gecikme:** Milyonlarca hesap olsa bile, bir hesabın değişmesi durumunda tüm state'in yeniden hashlenmesi gerekmez ($O(\log N)$ karmaşıklık).
2. **Disk Dostu:** Sadece değişen hesaplar diske yazılır (**Per-Account Persistence**). Eskiden tüm state koca bir JSON blob'u iken, artık her hesap `ACCT:{pubkey}` anahtarıyla ayrı ayrı kaydedilir.
3. **Merkle Proofs:** Hafif istemciler (Light Clients), tüm hesap verisini indirmeden sadece Merkle yolunu (Path) ve Root'u kullanarak bir hesabın bakiyesini kriptografik olarak doğrulayabilir.

### 2. Dinamik Parametreler ve Yönetişim (Hardening)

Budlum'un üretim sürümünde, ağ parametreleri artık statik kod sabitleri değil, state içinde yaşayan dinamik değişkenlerdir.

- **`base_fee`**: Ağdaki minimum işlem ücreti. Blok doluluğuna göre EIP-1559 benzeri bir elastik yapıyla güncellenir.
- **`block_reward`**: Validatörlere verilen blok üretim ödülü.
- **Yönetişim (Governance)**: `TransactionType::Vote` ile başlayan oylama süreçleri, epoch geçişlerinde (`advance_epoch`) sonuçlandırılır ve bu parametreler on-chain olarak güncellenir.

---

## 3. Fonksiyonlar ve İş Mantığı

### Fonksiyon: `validate_transaction` (Kural Kontrolü)

Bir işlemin geçerli olup olmadığına sadece kryptografik olarak değil, **ekonomik** olarak da karar verilir.

```rust
pub fn validate_transaction(&self, tx: &Transaction) -> Result<(), String> {
    // 1. İmza kontrolü (Transaction üzerindeki verify)
    if !tx.verify() { return Err("İmza geçersiz".into()); }

    // 2. Nonce Kontrolü (Sıra Takibi)
    let expected_nonce = self.get_nonce(&tx.from);
    if tx.nonce != expected_nonce {
        // "Senin sıradaki işlemin 5 olmalıydı ama sen 6 gönderdin (aradaki kayıp) 
        // veya 4 gönderdin (tekrar ediyorsun)".
        return Err(format!("Nonce hatası: Beklenen {}, Gelen {}", expected_nonce, tx.nonce));
    }

    // 3. Bakiye Kontrolü (Yetersiz Bakiye)
    let balance = self.get_balance(&tx.from);
    if balance < tx.total_cost() { // total_cost = amount + fee
        return Err("Yetersiz Bakiye".into());
    }

    // 4. Tip Kontrolleri (Stake, Unstake vb.)
    // Örneğin: Stake miktarı 0 olamaz, olmayan parayla stake yapılamaz.
    // ...
    Ok(())
}
```

**Neden Bu Sıra?**
En ucuz kontroller (imza, nonce) önce yapılır. Veritabanı okuması gerektiren veya daha karmaşık mantıklar sonra gelir. Hatayı ne kadar erken yakalarsak sistem o kadar az yorulur (Fail Fast).

---

### Fonksiyon: `apply_transaction` (Durum Değişikliği)

Tüm kontroller geçildikten sonra paranın el değiştirdiği yerdir.

```rust
pub fn apply_transaction(&mut self, tx: &Transaction) -> Result<(), String> {
    // 1. Gönderen hesabı bul ve parayı güvenli düş.
    let sender = self.get_or_create(&tx.from);
    sender.balance = sender.balance.saturating_sub(tx.total_cost());
    sender.nonce = sender.nonce.saturating_add(1);

    // 2. İşlem tipine göre davran.
    match tx.tx_type {
        TransactionType::Transfer => {
            let receiver = self.get_or_create(&tx.to);
            receiver.balance = receiver.balance.saturating_add(tx.amount);
        }
        TransactionType::Stake => {
            let validator = self.get_validator_mut(&tx.from);
            if let Some(v) = validator {
                v.stake = v.stake.saturating_add(tx.amount);
                v.active = true;
            } else {
                self.add_validator(tx.from, tx.amount);
            }
        }
        // ...
    }
    Ok(())
}
```

**Kritik Detay: `get_or_create`**
Blockchain'de hesap açmak için bankaya gidilmez. Biri size para yolladığında hesabınız o an oluşur. `get_or_create` fonksiyonu bu dinamikliği sağlar: "Hesap varsa getir, yoksa 0 bakiye ile yarat."

---

### Fonksiyon: `apply_block` (Blok Seviyesi State Determinism)

`apply_transaction` sadece tek bir işlemi uygularken, `apply_block` koca bir bloğun tüm işlemlerini sırayla işleterek state'i (bakiye durumunu) bir sonraki evreye geçirir.

**Güvenlik ve Determinism Kuralı:** Mainnet Hardening kapsamında, bu fonksiyon artık **hataları yutmaz**. `apply_block` işlemi bir `Result<(), String>` döner. Eğer blok içindeki tek bir işlem dahi (yetersiz bakiye veya hatalı nonce sebebiyle) `apply_transaction` aşamasında başarısız olursa, blok anında reddedilir ve süreç iptal edilir.
Ayrıca node ilk başlatılırken (`init` süreci) diskten okunan eski bloklar `apply_block` ile state'e uygulanırken bir hata alınırsa, node sessizce bozuk state ile ayağa kalkmak yerine **`std::process::exit(1)` ile kontrollü bir şekilde çöker (hard fail)**. Bu sayede veritabanı tutarsızlığı önlenir.

---

### Fonksiyon: `apply_slashing` (Adalet Dağıtımı)

Konsensüs motoru (PoS) bir suç tespit ettiğinde bu fonksiyonu çağırır. **Önemli:** Determinizm için tüm ekonomi hesaplamaları tam sayı (integer) aritmetiği ile yapılır.

```rust
pub fn apply_slashing(&mut self, evidences: &[SlashingEvidence], slash_ratio_scaled: u64) {
    for evidence in evidences {
        let producer = &evidence.header1.producer;
        
        if let Some(validator) = self.validators.get_mut(producer) {
            if !validator.slashed {
                // Fixed-point math: (stake * ratio) / SCALE
                let penalty = (validator.stake * slash_ratio_scaled) / FIXED_POINT_SCALE;
                validator.stake = validator.stake.saturating_sub(penalty);
                validator.slashed = true;
                validator.active = false;
                validator.jailed = true;
            }
        }
    }
}
```

**Satır Satır:**
- `saturating_sub`: Eğer ceza miktarı bakiyeden fazlaysa, sonuç eksiye düşmesin, 0 olsun diye kullanılır. Güvenli matematik işlemidir. Rust'ta *panic* (çökme) olmasını engeller.
- **Neden Stake Siliniyor?** Caydırıcılık. Eğer sadece sistemden atsaydık, paralarını çekip başka bir kimlikle geri gelirlerdi. Para kaybetmek en büyük korkudur.
