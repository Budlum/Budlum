# Bölüm 10: Benchmark Sonuçları

Bu bölüm, Budlum blok zincirinin farklı katmanlardaki performans metriklerini belgelemektedir.

## Mikro-Benchmark'lar
Bu testler, bireysel bileşenlerin teorik maksimum işlem kapasitesini ölçer.

| Bileşen | Metrik | Sonuç |
|-----------|--------|--------|
| **İmza Doğrulama** | Ed25519 (Sha3) | **~20,500 doğrulama/sn** |
| **Merkle Ağacı Güncelleme** | Artımlı (Incremental) O(log N) | **~1,450 güncelleme/sn** |

> [!IMPORTANT]
> **State Root Hesaplaması** şu anda sistemin ana darboğazıdır. Her transfer işlemi en az iki güncelleme (gönderici ve alıcı) gerektirdiğinden, sürdürülebilir transfer kapasitesi ~700 TPS ile sınırlıdır.

## Dahili Pipeline (Tek Düğüm)
Bu metrikler, ağ gecikmesi ve serileştirme yükünü devre dışı bırakarak tek bir düğümün uçtan uca performansını ölçer.

- **Test Yükü**: 10.000 Benzersiz Göndericiden 10.000 İşlem.
- **Ingest TPS (Mempool Giriş)**: **~8,900 işlem/sn**
- **Execution TPS (Blok İşleme)**: **~15,200 işlem/sn**
- **Başarı Oranı**: **%100**
- **Ortalama Blok Üretim Süresi**: **~650ms** (10.000 işlemlik blok için)

## Analiz ve Darboğazlar

### 1. İşlem Kabulü (Ingestion)
Mempool işlem kabulü şu an için `ChainActor` döngüsünün tek iş parçacıklı yapısı ve katı nonce doğrulaması ile sınırlıdır. 9k TPS yüksek bir değer olsa da, çoklu doğrulayıcı (multi-validator) kurulumlarına ölçeklenirken kabul hattının optimize edilmesi gerekecektir.

### 2. State Root Güncellemeleri
Merkle güncellemelerindeki 1.4k/sn limiti, state büyüdükçe ve çok sayıda benzersiz hesaba dokunuldukça blok üretim sürelerinin doğrusal olarak artacağını göstermektedir. Phase 3 için **Sparse Merkle Trees** veya **Paralel Hashing** gibi optimizasyon stratejileri değerlendirilmelidir.

### 3. Yürütme Gücü (Execution Power)
15k TPS yürütme hızı, çekirdek Rust işlem mantığının son derece verimli olduğunu kanıtlamaktadır. Yürütme Katmanı'nın (EVM/WASM) getireceği ek yük bir sonraki aşamada ölçülecektir.
