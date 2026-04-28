# Proof of Authority (PoA)

Proof of Authority (Yetki Kanıtı), kimlik tabanlı bir konsensüs mekanizmasıdır. Genellikle özel (private) veya konsorsiyum blok zincirlerinde kullanılır.

Budlum'un PoA uygulaması `src/consensus/poa.rs` dosyasındadır.

## Çalışma Mantığı

PoA'da "madencilik" veya "stake" yoktur. Bunun yerine, önceden belirlenmiş güvenilir düğümler (Otoriteler) vardır.

1.  **Yetkili Listesi:** Budlum PoA modunda yetkili adresler node açılışında `validators.json` dosyasından okunur ve state içindeki validator set'ine yüklenir.
2.  **Sıralı Üretim (Round Robin):** Otoriteler sırayla blok üretir.
    -   Örneğin 3 otorite varsa (A, B, C):
    -   Blok 1 -> A
    -   Blok 2 -> B
    -   Blok 3 -> C
    -   Blok 4 -> A ...

## Avantajları

-   **Yüksek Performans:** Karmaşık hesaplamalar (PoW) yoktur. Bloklar çok hızlı üretilir.
-   **Düşük Enerji:** Sadece basit imza doğrulama işlemi yapılır.
-   **Tahmin Edilebilirlik:** Blok üretim süreleri sabittir.

## Dezavantajları

-   **Merkeziyetçilik:** Ağın güvenliği, sınırlı sayıdaki otoriteye emanettir. Bu otoriteler işbirliği yaparsa ağı manipüle edebilirler.
-   **Sansür Riski:** Otoriteler, belirli işlemleri bloklara almayı reddedebilir.

Budlum PoA motoru, blok başlığındaki `producer` alanının state içindeki aktif validator listesinde olup olmadığını ve sırasının gelip gelmediğini kontrol eder.

## Uygulama Notları

- `--validators-file`: Yetkili adres listesini yükler.
- `--validator-key-file`: Lokal node'un imza anahtarını yükler.
- Node blok üretirken önce beklenen proposer adresini hesaplar, sonra lokal anahtar bu adrese aitse bloğu imzalar.
- CLI tarafındaki `mine`/`block` komutu, `--validator-address` verilmemişse mümkünse lokal signer adresini kullanır. Bu sayede ödül ve `producer` alanı imza atan otoriteyle uyumlu kalır.

Kısacası Budlum PoA artık sadece "doğrulamada round-robin" değil, node wiring seviyesinde de gerçek validator dosyası ve signer anahtarıyla çalışan bir moddur.
