# Bölüm 3.1: Konsensüs Motoru Arayüzü

Bu bölüm, blok zincirinin "beyni" olan konsensüs mekanizmasının nasıl soyutlandığını (`Trait`) ve modüler yapısını anlatır. Neden PoW ve PoS arasında tek satır kodla geçiş yapabiliyoruz?

Kaynak Dosya: `src/consensus/mod.rs` (Varsayımsal trait tanımı)

---

## 1. Veri Yapıları: Soyutlama (Abstraction)

Yazılım mimarisinde "Dependency Inversion" prensibi vardır. Budlum ana kodu (`Node`, `Blockchain`), doğrudan `PoW` veya `PoS` koduna bağlı değildir. Sadece `ConsensusEngine` arayüzüne (Trait) bağlıdır.

### Trait: `ConsensusEngine`

Tüm konsensüs motorlarının uyması gereken sözleşmedir.

```rust
pub trait ConsensusEngine: Send + Sync {
    // 1. Yeni blok hazırlanırken ne yapayım? (Mining / Minting)
    fn prepare_block(&self, block: &mut Block, state: &AccountState);

    // 2. Gelen blok geçerli mi? (Validation)
    fn validate_block(&self, block: &Block, chain: &[Block], state: &AccountState) -> bool;

    // 3. Çatallanma (Fork) durumunda hangisini seçeyim?
    fn fork_choice_score(&self, chain: &[Block]) -> u128;
}
```

**Analiz:**

| Fonksiyon | PoW'da Ne yapar? | PoS'da Ne yapar? | PoA'da Ne Yapar? |
| :--- | :--- | :--- | :--- |
| `prepare_block` | **Madencilik yapar.** `nonce` dener, CPU yakar. | **Lider kontrolü yapar.** "Sıra bende mi?" diye bakar, imza atar. | **Yetki kontrolü.** "Listede adım var mı?" diye bakar. |
| `validate_block`| **Hash kontrolü.** Hedef zorluğu tutturmuş mu? | **İmza kontrolü.** Blok üreticisi o slotun lideri mi? | **İmza kontrolü.** Yetkili listeden mi gelmiş? |
| `fork_choice` | **En Zor Zincir.** Toplam zorluk (Difficulty) kimde fazlaysa o kazanır. | **En Ağır Zincir.** Toplam stake kimde fazlaysa (veya LMD-GHOST) o kazanır. | **En Uzun Zincir.** Blok sayısı kimde fazlaysa. |

---

## 2. Tasarım Kararı: Neden Trait?

Budlum projesini başlatırken `main.rs` içinde şöyle bir seçim yapabiliriz:

```rust
// PoW kullanmak istersek:
let engine = Arc::new(PoWEngine::new(config));

// PoS kullanmak istersek:
// let engine = Arc::new(PoSEngine::new(config, keypair));

let node = Node::new(blockchain, engine); // Node'un umurunda değil!
```

Bu yapı sayesinde:
1.  **Test Edilebilirlik:** Testlerde `MockEngine` (Sahte Motor) kullanarak, madencilik yapmadan saniyesinde blok üretebilir ve diğer fonksiyonları test edebiliriz.
2.  **Esneklik:** Ağ çalışırken Hard Fork ile (yazılım güncellemesiyle) konsensüs değişimi yapılabilir (Tıpkı Ethereum'un Merge güncellemesi gibi).

---

## 3. Fork Choice Rule (Çatal Seçim Kuralı)

Blok zinciri bazen ayrışır. İki madenci aynı anda blok bulur.
-   Zincir A: Blok 0 -> Blok 1a
-   Zincir B: Blok 0 -> Blok 1b

Hangi zincir "gerçek" olandır?

`fork_choice_score` fonksiyonu buna puan verir. Düğümler (Nodes) her zaman **en yüksek puanlı** zinciri takip eder.
-   PoW'da bu puan, kümülatif zorluktur (Work).
-   PoS'ta bu puan, zincirdeki blokları imzalayanların toplam hissesidir (Weight).
