# Bölüm 2.1: Kriptografik Kimlik ve İmzalar

Bu bölüm, `src/crypto.rs` dosyasındaki kimlik oluşturma (Key Generation), imzalama (Signing) ve doğrulama (Verification) süreçlerini en ince detayına kadar açıklar. Blok zincirinde "şifre" yoktur, "özel anahtar" vardır.

Kaynak Dosya: `src/crypto.rs`

---

## 1. Veri Yapıları: Kimlik Kartımız

Blok zincirinde kullanıcı adı ve parola yoktur. Bunun yerine Asimetrik Kriptografi (Public/Private Key) kullanılır.

### Struct: `KeyPair`

Bu yapı, kullanıcının cüzdanıdır.

**Kod:**
```rust
pub struct KeyPair {
    // Ed25519 kütüphanesinin kendi Keypair yapısını sarmalıyoruz (Wrapper).
    inner: ed25519_dalek::Keypair,
}
```

**Analiz:**

| Alan Adı | Veri Tipi | Neden Bu Tipi Seçtik? | Ne İşe Yarar? |
| :--- | :--- | :--- | :--- |
| `inner` | `ed25519_dalek::Keypair` | **Ed25519 Algoritması.** RSA veya ECDSA (Bitcoin) yerine Ed25519 seçtik. Çünkü daha hızlıdır, daha küçük anahtarlar üretir (32 byte) ve yan kanal saldırılarına (Side-channel attacks) karşı daha dirençlidir. | **Anahtar Çifti.** İçinde hem Açık Anahtar (Public Key) hem de Özel Anahtar (Private Key) barındırır. |

---

## 2. Algoritmalar: İmzalama ve Doğrulama

### Fonksiyon: `new` (Rastgele Cüzdan Oluşturma)

Sıfırdan yeni bir hesap oluşturur.

```rust
pub fn new() -> Self {
    // 1. İşletim sisteminin güvenli rastgele sayı üretecini (CSPRNG) al.
    let mut csprng = OsRng {};
    
    // 2. Bu rastgelelik ile yeni bir anahtar çifti türet.
    let keypair = ed25519_dalek::Keypair::generate(&mut csprng);
    
    // 3. Sarmalayıp döndür.
    KeyPair { inner: keypair }
}
```

**Tasarım Notu (Entropy):**
Anahtar oluştururken `rand::thread_rng()` yerine `OsRng` kullanmak kritiktir. Eğer rastgele sayı tahmin edilebilir olursa (örneğin bilgisayarın o anki saatine bağlıysa), saldırgan aynı anahtarı kendi bilgisayarında üretip cüzdanınızı çalabilir. `OsRng`, donanım gürültüsünü (klavye tuşlamaları, fan hızı vb.) kullanarak tahmin edilemezlik (entropi) sağlar.

---

### Fonksiyon: `sign` (Dijital İmza Atma)

Bir veriye (mesaja) onay verdiğinizi kanıtlar.

```rust
pub fn sign(&self, message: &[u8]) -> Vec<u8> {
    // 1. Kütüphanenin sign fonksiyonunu çağır.
    let signature = self.inner.sign(message);
    
    // 2. İmzayı byte dizisine (64 byte) çevirip döndür.
    signature.to_bytes().to_vec()
}
```

**Nasıl Çalışır?**
`Ed25519` imzası deterministiktir. Yani aynı mesajı aynı anahtarla 100 kere imzalasanız, 100 kere aynı bit dizisini (array) elde edersiniz. (ECDSA'da bu rastgeledir). Bu özellik, test etmeyi ve hata ayıklamayı çok kolaylaştırır.
-   **Girdi:** Mesaj (Veri) + Özel Anahtar (Gizli)
-   **Çıktı:** İmza (64 Byte)

---

### Fonksiyon: `verify_signature` (Doğrulama)

Bu fonksiyon `Block::verify` ve `Transaction::verify` tarafından kullanılır.

```rust
pub fn verify_signature(
    message: &[u8],      // İmzalanan veri (Hash)
    signature: &[u8],    // İddia edilen imza
    public_key: &[u8]    // İddia edilen kişinin açık anahtarı
) -> Result<(), SignatureError> {
    // 1. Açık anahtarı byte dizisinden nesneye çevir. Hatalı format varsa reddet.
    let pk = PublicKey::from_bytes(public_key)?;

    // 2. İmzayı nesneye çevir. (64 byte değilse hata verir).
    let sig = Signature::from_bytes(signature)?;

    // 3. Matematiksel doğrulama yap.
    // Denklem: S * G = R + H(R, A, M) * A
    // (Burada A: Public Key, M: Mesaj, S,R: İmza parçaları)
    pk.verify(message, &sig)
}
```

**Neden Kritiktir?**
Blok zincirinde kimse kimseye güvenmez. Alice "Ben 10 coin yolladım" dediğinde (Transaction), Bob "Gerçekten Alice mi yolladı?" diye sormaz. Alice'in `Public Key`ini ve mesajın `Hash`ini bu denkleme sokar. Eğer denklem eşit çıkarsa, matematiksel olarak bunu sadece Alice'in `Private Key`i yapmış olabilir. Başka bir ihtimal evrenin yaşı kadar sürede bile denense bulunamaz.

---

### Fonksiyon: `public_key_hex` (Adres Formatı)

Açık anahtarı, insanların okuyabileceği ve paylaşabileceği bir formata ("Adres") çevirir.

```rust
pub fn public_key_hex(&self) -> String {
    // 1. Public Key'i byte dizisi olarak al (32 byte).
    let bytes = self.inner.public.to_bytes();
    
    // 2. Hexadecimal (16'lık taban) stringe çevir.
    // Örn: [255, 0, 10] -> "ff000a"
    hex::encode(bytes)
}
```

**Tasarım Kararı:**
Budlum projesinde adres olarak doğrudan `Public Key`in Hex hali kullanılır.
-   **Bitcoin:** `RIPEMD160(SHA256(PublicKey))` -> Base58 -> Adres (Daha kısa)
-   **Ethereum:** `Keccak256(PublicKey)[-20:]` -> Hex -> Adres
-   **Budlum:** `Hex(PublicKey)` (Daha basit, hashing yok).
    -   Avantaj: İşlem yükü (CPU) daha az.
    -   Dezavantaj: Adresler biraz daha uzun (64 karakter).

---

---

## 3. Hardening Phase 2: Yeni Kriptografik Şemalar

Projenin güvenlik seviyesini artırmak için klasik Ed25519 imzalarının yanına iki yeni şema eklenmiştir: **BLS** (Finalite için) ve **Dilithium** (Kuantum Sonrası Güvenlik için).

### 1. BLS İmzaları (Boneh-Lynn-Shacham)

BLS imzaları, birden fazla imzanın tek bir imza haline getirilebilmesi (Aggregation) özelliği için tercih edilmiştir.

- **Kullanım Alanı:** Finalite Katmanı oylamaları (Prevote/Precommit).
- **Avantajı:** 100 validatörün farklı imzalarını 96 byte'lık tek bir imzaya indirger. Bu, blok boyutunu ve doğrulama maliyetini (CPU) devasa oranda düşürür.
- **Teknik Detay:** `bls12_381` eğrisi kullanılır. İmzalar agregasyon sırasında G1 grubu üzerinde nokta toplama (point addition) işlemiyle birleştirilir: $S_{agg} = \sum P_i$.
- **Doğrulama:** Eşleme (Pairing) dostu eğriler sayesinde, agregasyon yapılmış tek bir imza, tüm imzalayanların açık anahtarlarının toplamı ($\sum PK_i$) kullanılarak tek bir operasyonla doğrulanabilir.

### 2. Dilithium (Post-Quantum Signature)

Kuantum bilgisayarların Ed25519 gibi klasik eliptik eğri şemalarını kırma potansiyeline karşı eklenmiş bir "Optimistic" güvenlik katmanıdır.

- **Kullanım Alanı:** Optimistic QC (PQ Attestation).
- **Neden Dilithium?** NIST tarafından kuantum sonrası standart olarak seçilmiştir. Budlum, ana zincir performansını düşürmemek için bu ağır imzaları (yüzlerce byte) ana blok içine değil, yan kanal olan `QcBlob` içine gömer.
- **Gerçek Doğrulama:** `src/crypto/primitives.rs` içindeki `PqKeyPair` yapısı, `pqcrypto-dilithium` üzerinden detached signature üretir ve doğrular.
- **Validator Identity:** Artık validator kaydı sadece BLS/VRF ile sınırlı değildir; aktif validator kimliği `pq_public_key` bilgisini de taşır.
- **Anahtar Paketi:** `ValidatorKeys` dosyaları Ed25519 imza anahtarı + VRF anahtarı + opsiyonel Dilithium anahtarını bir arada saklayabilir. Eski anahtar dosyaları geriye dönük olarak yüklenebilir, ancak PQ doğrulama gerektiren akışlarda `pq_public_key` eksik validator'lar geçerli sayılmaz.

### 3. Proof of Possession (PoP)

BLS şemasında saldırganların sahte anahtarlar üretmesini (Rogue-key attack) engellemek için kullanılan bir güvenlik mekanizmasıdır.

- **Mekanizma:** Bir validatör BLS anahtarını sisteme tanıtırken, o anahtarla kendi adresini imzaladığı bir **PoP** kanıtı sunmak zorundadır.
- **Doğrulama:** `verify_pop` fonksiyonu, validatörün hem Ed25519 hem de BLS anahtarının sahibi olduğunu doğrular.

---

## Özet

`src/crypto/primitives.rs` dosyası, tüm sistemin güvenliğinin dayandığı temeldir.
1.  **Hibrit Güvenlik:** Ed25519 (Hız) + BLS (Verimlilik) + Dilithium (PQ Güvenlik).
2.  **Ölçeklenebilirlik:** BLS agregasyonu sayesinde binlerce validatörle bile düşük ağ yükü.
3.  **Geleceğe Hazırlık:** Kuantum ötesi senaryolara bugünden hazır mimari.
4.  **State-Correctness Bağı:** PQ katmanı artık sadece “ek bir imza türü” değil; validator kimliği, finality gating ve fraud-proof doğrulamasıyla zincirin karar mekanizmasına bağlıdır.
