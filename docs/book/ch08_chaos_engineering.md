# Bölüm 8: Kaos Mühendisliği ve Testler

Budlum, "her şeyin ters gidebileceği" varsayımı üzerine inşa edilmiştir. Bu yüzden standart unit testlerin yanına, ağın dayanıklılığını ölçen **Kaos Testleri** eklenmiştir.

## 1. Test Kategorileri

| kategori | Dosya Yolu | Açıklama |
| :--- | :--- | :--- |
| **Unit Tests** | `src/**/*.rs` (iç içe) | Fonksiyon düzeyinde doğruluk testleri. |
| **Integration Tests** | `src/tests/integration.rs` | Birden fazla modülün (Consensus + Storage + Executor) uyumu. |
| **Chaos Tests** | `src/tests/chaos.rs` | Ağ kesintileri, geçersiz veri saldırıları, reorg senaryoları. |

## 2. Kaos Test Senaryoları

Şu anki altyapıda simüle edilen kaos durumları:

1. **Ağ Bölünmesi (Network Partitioning)**: Node'lar arası iletişimin kopması ve tekrar birleşmesi sonrası finalite korunması.
2. **Re-org Koruması**: Beklenenden daha derin bir blok değişiminin (Re-organization) sistem tarafından reddedilmesi.
3. **Senkronizasyon Bozulması**: Hatalı blok zinciri gönderen düğümlere karşı sistemin kendini koruması ve doğru zinciri bulması.

## 3. Testleri Çalıştırma

Tüm testleri çalıştırmak için:
```bash
cargo test
```

Sadece kaos testlerini çalıştırmak için:
```bash
cargo test tests::chaos
```

## 4. Felsefe: "Fail Early, Fail Safely"

Kodun her noktasında `assert!` ve `Result` tipleri yoğun kullanılarak, bir hata durumunda sistemin "tutarsız bir duruma" (Corrupted state) girmek yerine güvenli bir şekilde durması (Panic/Error) hedeflenmiştir.
