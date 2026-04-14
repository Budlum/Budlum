# Bölüm 3.5: Finalite Katmanı (BLS)

Bu bölüm, Budlum blok zincirinin "kesinlik" (finality) kazandığı **BLS Finalite Katmanı**'nı açıklar. Bu katman, uzun süreli zincir bölünmelerini (split) engeller ve saniyeler içinde geri alınamazlık garantisi verir.

Kaynak Dosyalar: `src/chain/finality.rs`, `src/chain/blockchain.rs`

---

## 1. Neden Finalite Katmanı?

Standart PoS veya PoW sistemlerinde bir bloğun "kesinleşmesi" için üzerine belirli sayıda blok eklenmesi beklenir (Örn. Bitcoin için 6 blok, Ethereum için 2 epoch). Budlum, **Hardening Phase 2** ile bu bekleme süresini optimize etmek ve güvenliği artırmak için ek bir oylama katmanı sunar.

### Temel Hedefler:
- **Hız:** 100 blokta bir (Checkpoint) anında kesinlik sağlar.
- **Güvenlik:** Kötü niyetli validatörlerin hisselerini anında slashing ile cezalandırır.
- **Değiştirilemezlik:** Finalize edilen bir bloktan geriye dönük (reorg) asla gidilemez.
- **PQ Bağlantısı:** BLS sertifikası tek başına yeterli değildir; imzalayan validator'ların ilgili `QcBlob` içindeki Dilithium attestasyonları da mevcut ve geçerli olmalıdır.

---

## 2. İki Aşamalı Oylama Protokolü

Finalite süreci, periyodik olarak (her 100 blokta bir) tetiklenir ve iki aşamadan oluşur:

### Aşama 1: Prevote
Validatörler, mevcut epoch'un son bloğunu (Checkpoint) inceler ve "Bu blok benim için geçerlidir" diyerek bir **BLS Prevote** imzası atar.
- **Kural:** Validatör setinin en az 2/3'ü Prevote verirse 1. aşama tamamlanır.

### Aşama 2: Precommit
Prevote çoğunluğu sağlandığında, validatörler ikinci bir onay oyu verir: **Precommit**. 
- **Kural:** En az 2/3 çoğunluk Precommit verirse, bu checkpoint blok zinciri tarihinde "Kalıcı" (Finalized) olarak işaretlenir.

---

## 3. Otomatik Oylama Döngüsü

**Hardening** kapsamında, validatörlerin manuel müdahalesine gerek kalmadan arka planda çalışan bir oylama mekanizması eklenmiştir:

- **Interval:** Her 30 saniyede bir tetiklenir.
- **Kontrol:** Eğer mevcut blok yüksekliği bir checkpoint ise ve henüz oy verilmemişse, otomatik olarak bir `Prevote` mesajı yayımlanır.
- **Ağ Duyurusu:** Oylar `blocks` Gossipsub kanalı üzerinden tüm ağa yayılır.

Bu sayede ağ, konsensüs sağlandığı sürece kendi kendine ilerlemeye (liveness) devam eder.

---

## 3. Veri Yapısı: `FinalityCert`

Oylamalar tamamlandığında, `FinalityAggregator` tüm imzaları birleştirerek tek bir sertifika oluşturur.

```rust
pub struct FinalityCert {
    pub epoch: u64,
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
    pub agg_sig_bls: Vec<u8>,    // G1 Projective nokta toplama ile üretilmiş aggregate imza
    pub bitmap: Vec<u8>,         // Hangi validatörlerin oy verdiğini gösteren bit dizisi
    pub set_hash: String,        // O anki validatör setinin özeti
}
```

### 3.1. Agregasyon Matematiği (Hardening)
Budlum'un üretim sürümünde imzalar sadece yan yana dizilmez (concatenation). `bls12_381` kütüphanesi kullanılarak G1 grubu üzerinde gerçek bir matematiksel toplama yapılır. Bu, sertifika boyutunun validatör sayısından bağımsız olarak her zaman sabit (96 byte) kalmasını sağlar.

### 3.2. QC Gating
`FinalityCert` kabulü artık yalnızca BLS aggregate signature doğrulaması değildir:

1. Checkpoint yüksekliği ve hash yerel zincirle eşleşir.
2. `ValidatorSetSnapshot` oluşturulur ve `set_hash` doğrulanır.
3. Sertifikanın bitmap'inden imzalayan validator indeksleri çıkarılır.
4. Aynı checkpoint için doğrulanmış `QC_BLOB` aranır.
5. `QcBlob`, signer coverage ile birlikte Dilithium imzaları açısından doğrulanır.

Bu sayede “BLS cert geçerli ama PQ sidecar eksik/bozuk” durumu finalize edilemez.

---

## 4. Slashing: `DoubleVote` (Ters Oylama)

Finalite katmanında en büyük suç, aynı epoch için iki farklı bloğa oy vermektir.

- **Senaryo:** Bir validatör hem A bloğuna hem de B bloğuna Precommit verirse, bu durum **Double Vote** suçunu oluşturur.
- **Tespit:** `verify_double_vote` fonksiyonu, bir kişinin aynı epoch için iki farklı hash imzaladığını kanıtlar.
- **Ceza:** Validatör derhal sistemden atılır ve bakiyesinin tamamı yakılabilir.

## 4.1. PQ Fraud ve Finality Invalidation

Finality katmanı artık sadece BLS double-vote suçlarını değil, checkpoint'i destekleyen hatalı PQ attestasyonlarını da hesaba katar.

- Eğer bir `PqFraudProof`, ilgili `QcBlob` içindeki bir yaprağın gerçekten geçersiz Dilithium imzası taşıdığını kanıtlarsa validator slash edilir.
- Aynı anda o checkpoint ve sonrasındaki finality kayıtları invalidation sürecine girebilir.
- Bu yaklaşım, “bir kez finalize olduysa artık her şey sorgusuz doğru” yerine “finality ancak tüm güvenlik katmanları tutarlıysa korunur” prensibini uygular.

---

## 5. Çatal Seçimi (Fork-Choice) ve Reorg Koruması

Blockchain motoruna eklenen yeni kural şudur:
> **Hiçbir düğüm, finalize edilmiş bir checkpoint bloğunun gerisindeki bir çatala geçiş yapamaz.**

- Eğer finalize edilmiş yükseklik 500 ise ve ağda 490. bloktan başlayan yeni bir çatal oluşursa, düğüm bu çatalın uzunluğu ne olursa olsun onu reddeder. 
- Bu sayede kullanıcılar, "Finalized" damgası yemiş bir işlemin asla geri alınmayacağından %100 emin olur (Immutability).

---

## Özet

BLS Finalite Katmanı, Budlum'u daha dirençli ve kurumsal kullanım için güvenli hale getirir.
1. **Verimlilik:** BLS ile binlerce imza tek bir sertifikada toplanır.
2. **Kesinlik:** Checkpoint'ler üzerinden reorg riski sıfıra indirilir.
3. **Ekonomik Güvenlik:** Double-vote kanıtları ile hile yapmanın maliyeti çok yüksektir.
4. **Katmanlı Doğrulama:** Finality artık BLS cert + validator set hash + doğrulanmış PQ blob kombinasyonuna dayanır.
