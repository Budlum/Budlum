# Bölüm 2: Kriptografi ve Güvenlik

"Kripto para" ismindeki "Kripto", kriptografiden (şifreleme bilimi) gelir. Ancak blok zincirlerinde genellikle mesajları gizlemek (encryption) değil, kimlik doğrulamak (signing) ve bütünlüğü sağlamak (hashing) için kullanılır.

Budlum projesinde kullanılan temel kriptografik algoritmalar şunlardır:

1.  **Ed25519:** Dijital imzalar için. Hızlı ve güvenli.
2.  **SHA3-256 (Keccak):** Veri özetleme (Hashing) için.
3.  **Merkle Ağaçları:** Veri bütünlüğünü kanıtlamak için.

Bu bölümde, bu algoritmaların blok zinciri bağlamında nasıl kullanıldığını göreceğiz.
