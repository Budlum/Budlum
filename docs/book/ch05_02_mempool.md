# Bölüm 5.2: İşlem Havuzu (Mempool) Mekaniği

Bu bölüm, ağa gelen işlemlerin bloklara girmeden önce beklediği "Bekleme Odası" olan Mempool'u, ücret piyasasını (Fee Market), sıralama algoritmalarını ve işlemlerin ağda nasıl yayıldığını (Gossip) analiz eder.

Kaynak Dosya: `src/mempool.rs`
---

## 1. Veri Yapıları: Çoklu Sıralama

Mempool, basit bir liste değildir. İşlemleri farklı kriterlere göre hızlıca bulabilmemiz gerekir.

### Struct: `Mempool`

```rust
pub struct Mempool {
    // Tüm işlemlerin ana deposu (Hash -> Tx)
    transactions: HashMap<String, Transaction>,

    // Gönderene göre işlemler (Sıralı).
    // Hangi gönderen, hangi nonce sırasına göre işlem atmış?
    // Alice -> [Nonce 5, Nonce 6, Nonce 7]
    by_sender: HashMap<String, BTreeMap<u64, String>>,

    // Ücrete göre işlemler (Sıralı).
    // Madenciler en çok para vereni seçmek ister.
    // Fee 100 -> [TxA, TxB], Fee 50 -> [TxC]
    by_fee: BTreeMap<u64, HashSet<String>>,
}
```

**Neden 3 Farklı Yapı?**
-   `transactions`: İşlemin detayına hızlı erişim (O(1)).
-   `by_sender`: Aynı kullanıcıdan gelen işlemlerin sırasını (Nonce) korumak için. Nonce 5 gelmeden Nonce 6 işlenemez.
-   `by_fee`: Bloğa sığacak en kârlı işlemleri (Greedy Algorithm) seçmek için.

---

## 2. Algoritmalar: Seçim ve Temizlik

### Fonksiyon: `add_transaction` (Kabul Salonu)

```rust
pub fn add_transaction(&mut self, tx: Transaction) -> Result<()> {
    // 1. Zaten var mı?
    if self.transactions.contains_key(&tx.hash) { return Err(...); }

    // 2. Havuz dolu mu? (DDoS Koruması)
    if self.transactions.len() >= self.config.max_size {
        // Havuz doluysa, gelen işlem mevcut en düşük ücretli işlemden 
        // daha mı değerli?
        let min_fee = *self.by_fee.keys().next().unwrap();
        
        if tx.fee > min_fee {
            // Evet daha değerli. Fakir olanı at, zengini al.
            self.evict_lowest_fee();
        } else {
            // Hayır, yeterince para vermemiş. Reddet.
            return Err("Mempool full and fee too low");
        }
    }

    // 3. İndeksleri güncelle.
    self.transactions.insert(tx.hash.clone(), tx.clone());
    self.by_fee.entry(tx.fee).or_default().insert(tx.hash.clone());
    // ...
}
```

### Fonksiyon: `get_sorted_transactions` (Madenci Seçimi)

Madenci blok oluştururken bu fonksiyonu çağırır.

```rust
pub fn get_sorted_transactions(&self, limit: usize) -> Vec<Transaction> {
    let mut selected = Vec::new();
    
    // En yüksek ücretliden (BTreeMap sondan başlar) en düşüğe doğru gez.
    // (Rust BTreeMap keys are sorted ascending, so rev() gives descending)
    for (fee, hashes) in self.by_fee.iter().rev() {
        for hash in hashes {
            if selected.len() >= limit { break; }
            
            // İşlemi al
            let tx = self.transactions.get(hash).unwrap();
            
            // Basitlik için Nonce sırasını şimdilik göz ardı ediyoruz
            // ama gerçekte burada Nonce gap kontrolü de yapılmalı.
            selected.push(tx.clone());
        }
    }
    selected
}
```

---

## 3. RBF (Replace By Fee)

Kullanıcı işleminin takıldığını görürse, aynı nonce ile **daha yüksek ücretli** yeni bir işlem gönderebilir.
`add_transaction` içinde bunu kontrol ederiz:
1.  Gönderenin aynı nonce'lu işlemi var mı?
2.  Varsa, yenisinin ücreti eskisinden %10 fazla mı?
3.  Fazlaysa eskisini sil, yenisini ekle.

Bu mekanizma, "Takılan işlemi kurtarma" (Unsticking Transaction) olarak bilinir.

---

## 4. Çöp Toplama (Garbage Collection / GC)

İşlemler sonsuza kadar Mempool'da bekleyemez. Aksi takdirde, ağda düşük ücretli milyonlarca spam işlem bellek (RAM) şişmesine (OOM) yol açar.

### Fonksiyon: `cleanup_expired`

Mempool, yapılandırmasında (`MempoolConfig`) bir "Yaşam Süresi" (TTL) barındırır. Arka planda Node'un `tokio::time::interval` görevleri tarafından periyodik (örn. 60 sn'de bir) olarak çağrılır.

```rust
pub fn cleanup_expired(&mut self) -> usize {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    
    // Süresi dolan işlemlerin hash'lerini bul
    let mut expired = Vec::new();
    for (hash, entry) in self.transactions.iter() {
        if now - entry.received_at > self.config.tx_ttl_secs {
            expired.push(hash.clone());
        }
    }

    // Hash'leri kullanarak işlemleri bellekten sil
    let count = expired.len();
    for hash in expired {
        self.remove_transaction(&hash);
    }
    
    count // Temizlenen işlem sayısını döndür
}
```

Bu periyodik temizleyici sayesinde ağ, kendi hafızasını (Mempool'u) otomatik ve sistemli olarak sürekli temizler.

## 5. Mempool Persistence (Disk Yedeği) ve Dayanıklılık

Normalde Mempool sadece RAM'dedir. Node kapandığında içindeki tüm bekleyen işlemler silinir. **Budlum Hardening Phase 2** ile birlikte artık tam kapsamlı **Mempool Persistence** ve hata denetimi (Error Handling) devrededir.

### Mekanizma:
1. **Atomic Save-on-Arrival:** Bir işlem Mempool'a eklendiğinde aynı anda veritabanına da (`MEM_TX:{hash}`) yazılır. Bu işlem atomik olarak gerçekleşir.
2. **Remove-on-Mined:** İşlem bir bloğa girdiğinde veya süresi dolduğunda (`cleanup_expired`) diskten de temizlenir.
3. **Startup Recovery:** Node açılırken `Blockchain::new()` fonksiyonu diskteki tüm `MEM_TX:` önekli işlemleri tarar ve Mempool'u otomatik olarak doldurur.
4. **Unwrap Audit (Güvenlik):** Mempool içindeki tüm `unwrap()` ve `expect()` çağrıları temizlenmiştir. Geçersiz bir işlem veya veritabanı hatası durumunda sistem çökmek (panic) yerine hatayı loglar ve güvenli bir şekilde çalışmaya devam eder.

Bu sayede beklenmedik kapanmalarda kullanıcı işlemleri ağda kaybolmaz ve sistem dış saldırılara karşı çok daha dirençli hale gelir.

---

## 6. İşlem Yayılımı (Transaction Gossip)

Bir işlem sadece tek bir node'da kalmaz. Tüm ağın bu işlemden haberdar olması gerekir ki, herhangi bir madenci onu bloğuna alabilsin.

**Mekanizma:**
1. **RPC Alımı:** Kullanıcı `bud_sendRawTransaction` ile bir işlem gönderir.
2. **Lokal Kayıt:** Node işlemi kendi Mempool'una ekler.
3. **Yayın (Broadcast):** İşlem geçerliyse, Node bunu `transactions` Gossipsub kanalına fırlatır.
4. **Zincirleme Etki:** Bu mesajı alan diğer nodelar da işlemi doğrular ve kendi komşularına iletir.

Bu sayede saniyeler içinde bir işlem dünyanın öbür ucundaki bir düğümün Mempool'una ulaşmış olur.
