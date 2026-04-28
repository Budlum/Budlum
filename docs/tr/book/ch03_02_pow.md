# Bölüm 3.2: Proof of Work (PoW) Motoru

Bu bölüm, Bitcoin'in de kullandığı İş Kanıtı (Proof of Work) mekanizmasının Budlum'daki uygulamasını, zorluk ayarlama algoritmasını ve madencilik döngüsünü satır satır analiz eder.

Kaynak Dosya: `src/consensus/pow.rs`

---

## 1. Veri Yapıları: Maden Ayarları

PoW, dinamik zorluk seviyesine sahip bir yarışmadır.

### Struct: `PoWConfig`

Madenciliğin kurallarını belirler.

**Kod:**
```rust
pub struct PoWConfig {
    pub difficulty: usize,        // Başlangıç zorluğu
    pub target_block_time: u64,   // Hedef: 1 blok kaç saniyede çıkmalı? (10sn)
    pub adjustment_interval: u64, // Zorluk kaç blokta bir güncellensin? (100)
    pub block_reward: u64,        // Blok bulanın ödülü
}
```

**Analiz:**

| Alan Adı | Veri Tipi | Varsayılan (Budlum) | Ne İşe Yarar & Neden Gerekli? |
| :--- | :--- | :--- | :--- |
| `difficulty` | `usize` | `4` | **Zorluk.** Hash'in kaç tane "0" ile başlaması gerektiğini belirtir. Sayı arttıkça madencilik üstel olarak (exponentially) zorlaşır. |
| `target_block_time` | `u64` | `10` sn | **Kalp Atışı.** Ağın ne kadar hızlı ilerlemesini istiyoruz? Çok hızlı olursa (1sn) çok fazla çatal (fork) oluşur. Çok yavaş olursa (10dk) kullanıcılar bekler. 10 saniye modern bir dengedir. |
| `adjustment_interval`| `u64` | `100` blok | **Zorluk Ayar Dönemi.** Her blokta zorluk değiştirmek sistemi kararsız (volatile) yapar. Bitcoin'de bu 2016 bloktur. Biz 100 seçtik ki ağ gücü değişimlerine hızlı tepki versin. |

---

### Struct: `PoWEngine`

Konsensüs motorunun kendisidir.

**Kod:**
```rust
pub struct PoWEngine {
    config: PoWConfig,
}
```
State (durum) tutmaz (`ConsensusEngine` özelliği gereği `stateless` olabilir), çünkü PoW tarihsel veriye ihtiyaç duymaz, sadece o anki bloğun hash'ine bakar. (Not: Zorluk ayarı için zincire bakar ama bunu fonksiyon argümanı olarak alır).

---

## 2. Algoritmalar: Madencilik ve Zorluk

### Fonksiyon: `mine` (Kazma İşlemi)

CPU'yu %100 kullanan o meşhur döngü.

```rust
pub fn mine(&self, block: &mut Block) {
    // 1. Hedef String'i oluştur.
    // Eğer zorluk 4 ise target = "0000"
    let target = self.target();
    
    println!("⛏️ Madencilik başladı! Hedef: {}...", target);

    // 2. Hash, hedefle başlayana kadar DÖN.
    while !block.hash.starts_with(&target) {
        // 3. Deneme sayacını artır.
        block.nonce += 1;
        
        // 4. Hash'i tekrar hesapla.
        // change(nonce) -> change(header) -> change(hash)
        block.hash = block.calculate_hash();

        // 5. Kullanıcıyı bilgilendir (Her 100.000 denemede bir)
        if block.nonce % 100000 == 0 {
            print!(".");
            io::stdout().flush().unwrap();
        }
    }
    // 6. Döngü bittiyse blok bulunmuştur!
    println!("✅ BLOK BULUNDU! Nonce: {}, Hash: {}", block.nonce, block.hash);
}
```

**Neden `nonce`?**
Bir bloğun içeriği (işlemler) sabittir. Sabit girdinin hash'i de sabittir. Farklı hashler elde etmek için, içeriği bozmayacak bir değişkene ihtiyacımız vardır. Bu "anlamsız sayı"ya `nonce` denir.

---

### Fonksiyon: `calculate_new_difficulty` (Zorluk Ayarı)

Ağın stabilitesini sağlayan en kritik algoritma. Eğer bilgisayarlar hızlanırsa zorluk artmalı, yavaşlarsa azalmalı.

```rust
pub fn calculate_new_difficulty(&self, chain: &[Block]) -> usize {
    // 1. Yeterli veri yoksa (Zincir başı), sabit zorluk dön.
    if chain.len() < self.config.adjustment_interval as usize {
        return self.config.difficulty;
    }

    // 2. Son 'interval' periyodunun başlangıcını ve sonunu bul.
    let last_block = chain.last().unwrap();
    let start_block = &chain[chain.len() - self.config.adjustment_interval as usize];

    // 3. Geçen GERÇEK süreyi ölç.
    let actual_time = last_block.timestamp - start_block.timestamp; // ms cinsinden
    let actual_seconds = actual_time / 1000;

    // 4. OLMASI GEREKEN süreyi hesapla.
    // 100 blok * 10 saniye = 1000 saniye sürmeliydi.
    let expected_seconds = self.config.adjustment_interval * self.config.target_block_time;

    // 5. Karşılaştır ve Ayarla.
    if actual_seconds < expected_seconds / 2 {
        // Çok hızı! (Yarım sürede bitmiş). Zorluğu 1 ARTIR.
        return self.config.difficulty + 1;
    } else if actual_seconds > expected_seconds * 2 {
        // Çok yavaş! (İki katı sürmüş). Zorluğu 1 AZALT.
        // Ama 1'in altına düşürme.
        if self.config.difficulty > 1 {
            return self.config.difficulty - 1;
        }
    }

    // Değişime gerek yok.
    self.config.difficulty
}
```

**Tasarım Kararı: Sınırlandırma (Damping)**
Algoritmada `expected_seconds / 2` ve `expected_seconds * 2` sınırları vardır.
-   Zorluk hemen değişmez. Sadece madencilik hızı **2 katına çıkarsa** veya **yarıya düşerse** müdahale edilir. Bu, zorluğun sürekli titremesini (oscillation) engeller.

---

### Fonksiyon: `validate_block` (Doğrulama)

Ağdan gelen bir bloğun geçerli olup olmadığını kontrol eder.

```rust
fn validate_block(&self, block: &Block, ...) -> bool {
    // 1. Hash doğru hesaplanmış mı? (Veri Bütünlüğü)
    if block.calculate_hash() != block.hash { return false; }

    // 2. Hash yeterince küçük mü? (Proof of Work)
    if !self.meets_difficulty(&block.hash) { return false; }

    // ...
    true
}
```

**Güvenlik:**
Her düğüm (Node), gelen her bloğu **tekrar hashler** ve doğrular. "Başkası doğrulamıştır" varsayımı (Trust) yoktur. Herkes kendi doğrulamasını yapar (Verify).
