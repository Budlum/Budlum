# Bölüm 2.2: Merkle Ağaçları ve Veri Bütünlüğü

Bu bölüm, blok başlığındaki `tx_root` ve `state_root` alanlarının nasıl hesaplandığını, "Hash Ağacı" (Merkle Tree) matematiğini ve hafif istemcilerin (Light Clients) bu yapıyı nasıl kullandığını anlatır.

---

## 1. Kavramsal Temel: Neden Ağaç?

Bir blokta 1 milyon işlem olduğunu düşünün.
-   **Senaryo:** Alice, "Benim işlemim bu blokta var mı?" diye soruyor.
-   **Kötü Çözüm (Düz Liste):** Bloktaki 1 milyon işlemi tek tek indir ve Alice'in işlemini ara. (1 GB veri indirmek gerekir).
-   **Merkle Çözümü:** Sadece Alice'in işleminden kök hash'e giden yolu (Path) indir. (Sadece 1 KB veri gerekir).

### Matematiksel Yapı

Merkle Ağacı, `Hash(Hash(A) + Hash(B))` şeklinde yukarı doğru çıkan bir piramittir. En tepedeki hash'e **Merkle Root** denir.

```text
       ROOT (H7)
      /       \
    H5         H6
   /  \       /  \
 H1    H2   H3    H4
 |     |    |     |
Tx1   Tx2  Tx3   Tx4
```

Eğer `Tx1` değişirse -> `H1` değişir -> `H5` değişir -> `ROOT` değişir.
Kök hash, altındaki milyonlarca yaprağın (Leaf) kriptografik özetidir.

---

## 2. Kod Analizi (`calculate_tx_root`)

Kodumuzda `src/block.rs` içinde `calculate_tx_root` fonksiyonu bu işlemi yapar.

```rust
pub fn calculate_tx_root(&self) -> String {
    // 1. Yaprakları Hazırla: Her işlemin kendi hash'ini al.
    let mut tx_hashes: Vec<String> = self.transactions
        .iter()
        .map(|tx| tx.hash.clone())
        .collect();

    // 2. Boş Blok Kontrolü
    if tx_hashes.is_empty() {
        return "0".repeat(64); // Standart boş kök.
    }

    // 3. Ağacı yukarı doğru ör.
    while tx_hashes.len() > 1 {
        let mut next_level = Vec::new();
        
        // Chunk(2): Listeyi ikişerli gruplara ayır. [A, B], [C, D]...
        for chunk in tx_hashes.chunks(2) {
            let left = &chunk[0];
            
            // Eğer sayı tekse (eşsiz kaldıysa), son elemanı kopyala (A, B, C -> C+C).
            // Bitcoin de böyle yapar.
            let right = if chunk.len() > 1 { &chunk[1] } else { left };

            // 4. İkisini birleştir ve hashle.
            // H(Left + Right)
            let combined = format!("{}{}", left, right);
            let new_hash = hex::encode(hash(combined)); // SHA3-256
            
            next_level.push(new_hash);
        }
        
        // Bir üst kata çık.
        tx_hashes = next_level;
    }

    // 4. Piramidin tepesi (Root).
    tx_hashes[0].clone()
}
```

---

## 3. Light Client (Hafif İstemci) Mantığı

Bir cep telefonu cüzdanı (SPV Wallet) nasıl çalışır?

1.  **Sadece Başlıkları İndir:** Blok başına 1 KB. 10 yıllık zincir bile 50 MB tutar.
2.  **Kök Kontrolü:** Başlıktaki `tx_root` elimizde.
3.  **Kanıt İste:** Full Node'a sor: "Tx1 bu root'un altında mı?"
4.  **Merkle Proof:** Full Node, Tx1'den Root'a giden yolu (`H2`, `H6`) gönderir.
5.  **Yerel Doğrulama:**
    -   Telefon hesaplar: `H1 = Hash(Tx1)`
    -   `H5 = Hash(H1 + H2)` (H2 ağdan geldi)
    -   `Hesaplanan_Root = Hash(H5 + H6)` (H6 ağdan geldi)
    -   Eğer `Hesaplanan_Root == Header.tx_root` ise, işlem %100 buradadır.

Bu sayede 1 TB'lık blok zincirini indirmeden, işlemler kriptografik kesinlikle doğrulanabilir.

---

## 4. Hardening Phase 2: QC Blob ve PQ İmzaları

Yeni eklenen Optimistic QC (Optimistik Çeyrek Sertifika) yapısında, Merkle ağaçları bu sefer kuantum sonrası (PQ) güvenliği optimize etmek için kullanılır.

### Problem: Devasa İmzalar
Dilithium imzaları Ed25519'dan kat kat daha büyüktür (birkaç KB). Eğer her validatörün PQ imzasını blok içine koysaydık, her blok MB'larca boyutunda olurdu.

### Çözüm: İmza Merkle Ağacı
Budlum, bu imzaları blok dışındaki `QcBlob` içinde Merkle ağacı yapısıyla saklar:
1. Her validatörün Dilithium imzası bir yapraktır (Leaf).
2. Bu imzalar bir ağaç oluşturur.
3. Ağacın kök hash'i (Root), blok başlığına eklenir.

**Neden Bu Yapıyı Seçtik?**
- **Optimistic Doğrulama:** Full nodelar bloğu hemen kabul eder. Eğer `QcBlob` içindeki bir PQ imzasının hatalı olduğu iddia edilirse, sadece o imza ve Merkle yolu (Path) kullanılarak **Fraud Proof** (Sahtecilik Kanıtı) sunulur.
- **Düşük Depolama:** Ana zincir şişmez; sadece kuantum saldırısı riski doğduğunda bu kanıtlar önem kazanır.

---

## 5. Budlum Hardening: Incremental State Merkle Tree

Normal Merkle ağaçları basittir ama her değişimde tüm ağacın en baştan yapraklarını (leaves) dizip hashlemesini gerektirir. Budlum Hardening ile birlikte **State Merkle Tree** artık artımlı (incremental) çalışır.

### Dirty Tracking ve Caching
1. **Dirty Kontrolü:** Hesaptaki her değişim (`balance`, `nonce`) o hesabı "kirli" (dirty) olarak işaretler.
2. **Yaprak Önbelleği:** Tüm yaprak hashleri bellekte (`cached_leaves`) saklanır.
3. **Kısmi Güncelleme:** `calculate_state_root` çağrıldığında:
   - Sadece *dirty* olan yaprakların hashleri yeniden hesaplanır.
   - Önbellekteki karşılıkları güncellenir.
   - Ağacın sadece o yapraktan Root'a giden dalı (branch) yeniden hesaplanır.

Bu sayede, 1 milyon hesaplık bir ağaçta tek bir hesap değiştiğinde 1 milyon hash işlemi yapmak yerine sadece $\approx 20$ hash işlemi yapılarak yeni Root bulunur.

## Özet

Merkle Ağaçları, Budlum projesinde sadece veri bütünlüğü değil, aynı zamanda **hız (Light Client)** ve **gelecek güvenliği (PQ Optimization)** sağlar.
1.  **İşlemler:** `tx_root` ile kanıtlanır (Standart Merkle).
2.  **Hesaplar:** `state_root` ile anlık bakiye doğrulanır. **Budlum Hardening** ile birlikte artık $O(\log N)$ maliyetli **Incremental Merkle Tree** yapısı kullanılır.
3.  **PQ Güvenlik:** `QcBlob` kökü ile devasa imzalar yönetilebilir hale gelir.
4.  **Hafif İstemciler:** Tüm state'i bilmeden saniyeler içinde bakiye kanıtı (Merkle Proof) yapabilirler.

