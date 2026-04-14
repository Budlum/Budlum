# Bölüm 5.1: Kalıcı Depolama (Sled DB)

Bu bölüm, verilerin RAM'den diske nasıl aktarıldığını, `sled` veritabanı yapısını ve "Key-Value" tasarımını analiz eder.

Kaynak Dosya: `src/storage/db.rs`

---

## 1. Veri Yapıları: Sled Nedir?

Budlum, `SQL` (Tablolar) yerine `NoSQL` (Key-Value) kullanır. Gömülü (Embedded) bir veritabanı olan **Sled** seçilmiştir.

### Neden Sled?
1.  **Gömülü:** Kurulum gerektirmez (PostgreSQL kurulumu gerekmez). Kodun içindedir. Programla birlikte derlenir.
2.  **Hızlı:** Modern NVMe diskler için optimize edilmiştir.
3.  **Thread-Safe:** Aynı anda birçok thread okuma/yazma yapabilir.

### Struct: `Storage`

```rust
#[derive(Clone)] // Clone ucuzdur, sadece dosya tanıtıcısını kopyalar.
pub struct Storage {
    db: Db, // Sled veritabanı handle'ı
}
```

---

## 2. Şema Tasarımı (Schema Design)

Veritabanında tablolar yoktur, sadece Anahtarlar (Key) ve Değerler (Value) vardır. Düzen sağlamak için **Prefix (Önek)** kullanırız.

| Veri Tipi | Anahtar Formatı (Key) | Değer (Value) | Açıklama |
| :--- | :--- | :--- | :--- |
| **Blok** | `{Hash}` | `Serialized(Block)` | Blok verilerini hash ile saklarız. |
| **Yükseklik** | `HEIGHT:{Number}` | `Hash` | Indexing: Numaradan Hash bulmak için. |
| **İşlem** | `TX_IDX:{Hash}` | `u64` | Indexing: Hash'ten Blok Numarası bulmak için. |
| **Hesap** | `ACCT:{PubKey}` | `Serialized(Account)` | Granular bakiye ve nonce saklama. |
| **Mempool** | `MEMPOOL:{Hash}` | `Serialized(Tx)` | Persistence: Bekleyen işlemlerin disk yedeği. |
| **QC Blob** | `QC_BLOB:{Height}` | `Serialized(QcBlob)` | Audit: Checkpoint imzalarının yedeklenmesi. |
| **Sertifika**| `FINALITY_CERT:{Height}` | `Serialized(FinalityCert)` | Proof: Finalize edilmiş blokların kanıtı. |
| **State Root** | `STATE_ROOT:{Height}` | `Hash` | Her canonical blok için state root kaydı. |
| **Canonical Height** | `CANONICAL_HEIGHT` | `u64` | Zincirin canonical ucunu gösteren yükseklik. |
| **Son Blok**| `LAST` | `Hash` (String) | Zincirin en ucunu (Tip) gösteren işaretçidir. |

---

## 3. Kod Analizi

### Fonksiyon: `commit_block` (Atomic Batching)

Budlum Hardening aşamasında, blok yazma işlemi artık **atomik**tir. Bir blok yazılırken elektrik kesilirse, ne blok ne de onunla ilgili indexler (yükseklik, tx index) yarım yamalak yazılmaz.

```rust
pub fn commit_block(&self, block: &Block, state_root: &str) -> io::Result<()> {
    let mut batch = sled::Batch::default();
    
    // 1. Bloğu hazırla
    let serialized = serde_json::to_vec(block)?;
    batch.insert(block.hash.as_bytes(), serialized);
    
    // 2. Yükseklik indexini hazırla
    batch.insert(format!("HEIGHT:{}", block.index), block.hash.as_bytes());
    batch.insert("LAST", block.hash.as_bytes());

    // 3. State Root ve TX indexlerini hazırla
    batch.insert(format!("STATE_ROOT:{}", block.index), state_root.as_bytes());
    for tx in &block.transactions {
        batch.insert(format!("TX_IDX:{}", tx.hash), block.index.to_string().as_bytes());
    }

    // 4. KRİTİK: Hepsini tek seferde (Atomik) diske yaz.
    self.db.apply_batch(batch)?;
    self.db.flush()?;

    Ok(())
}
```

### 4. Per-Account Persistence (Parçalı Kayıt)

Eskiden tüm bakiye state'i tek bir devasa JSON dosyası gibi saklanırdı. Bu, 1 milyon kullanıcı olduğunda tek bir bakiye değişse bile 100 MB veri yazmak demekti.

**Yeni Mimari:**
- Her hesap `ACCT:{PubKey}` anahtarı altında bağımsız bir K-V çiftidir.
- `Storage::save_account`: Sadece değişen hesabı diske yazar.
- `Storage::load_all_accounts`: Program açılırken `scan_prefix("ACCT:")` ile tüm hesapları diskten hızlıca toplar.

Bu sayede I/O maliyeti 1000 kat düşürülmüştür.

### Fonksiyon: `load_chain` (Başlangıç Yüklemesi)

Program açıldığında zinciri disken okur.

```rust
pub fn load_chain(&self) -> Vec<Block> {
    let mut chain = Vec::new();

    // 1. En son nerede kaldığımızı öğren.
    // "LAST" anahtarına bak.
    if let Some(last_hash_bytes) = self.db.get("LAST").unwrap() {
        let mut current_hash = String::from_utf8(last_hash_bytes.to_vec()).unwrap();

        // 2. Geriye doğru (Backtracking) yürü.
        loop {
            // Hash ile bloğu getir.
            let block = self.get_block(&current_hash).unwrap();
            
            // Önceki hash'i kaydet.
            let prev_hash = block.previous_hash.clone();
            
            // Zincire ekle.
            chain.push(block);

            // Eğer Genesis ise (Hash=000...) dur.
            if prev_hash == "0".repeat(64) {
                break;
            }
            current_hash = prev_hash;
        }
    }
    
    // 3. Tersten geldiğimiz için listeyi düzelt.
    chain.reverse();
    chain
}
```

**Tasarım Notu:**
Blockchain, aslında bir **Linked List** (Bağlı Liste) veri yapısıdır. Veritabanında her eleman bir öncekini işaret eder. Bu fonksiyon, bu bağlı listeyi takip ederek bütün zinciri yeniden inşa eder.

## 5. Reorg Sonrası Metadata Tutarlılığı

Budlum'da reorg artık sadece blok gövdelerini yazmakla kalmaz; canonical metadata da güncellenir.

- Eski dalın `HEIGHT`, `STATE_ROOT`, `FINALITY_CERT`, `QC_BLOB` kayıtları temizlenir.
- O dallara ait `TX_IDX` girişleri silinir.
- Yeni canonical dal `commit_block` üzerinden yeniden yazılır.
- `LAST` işaretçisi yeni ucun hash'ine taşınır.

Bu önemli bir ayrıntıdır, çünkü aksi halde node yeniden başlatıldığında disk üzerinde eski canonical bilgi ile yeni chain body birbirine karışabilir.
