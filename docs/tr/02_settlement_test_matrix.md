# Yerleşim Katmanı Test Matrisi ve Mimari

Bu döküman, Çoklu Konsensüs Yerleşim Katmanı'nın doğrulama durumunu takip eder ve mimari bir genel bakış sunar.

## 1. Test Matrisi

| Test Adı | Özellik | Durum |
|-----------|----------|--------|
| `test_cross_domain_double_spend_protection` | Paylaşımlı durum güvenliği | ✅ Geçti |
| `test_parallel_cross_domain_stress_determinism` | Stres determinizmi | ✅ Geçti |
| `test_async_gossip_random_delay_duplicate_drop` | Gossip yakınsaması | ✅ Geçti |
| `test_frozen_domain_persistence` | Bizans durum kalıcılığı | ✅ Geçti |
| `test_adversarial_finality_proofs` | Kesinlik kanıtı doğrulaması | ✅ Geçti |
| `test_restart_pending_buffer_persistence` | Çökme sonrası kurtarma | ✅ Geçti |
| `test_distributed_gossip_convergence` | Gerçek düğüm yakınsaması | ✅ Geçti |

## 2. Mimari Diyagram

```mermaid
graph TD
    subgraph "Konsensüs Domainleri"
        D1[PoW Domain]
        D2[PoS Domain]
        D3[ZK Domain]
    end

    subgraph "Budlum Düğümü (Yerleşim Katmanı)"
        Registry[Domain Kaydı]
        Buffer[Bekleme Tamponu]
        Verifier[Kesinlik Doğrulayıcı]
        State[Küresel Hesap Durumu]
        Storage[(Sled DB Kalıcılık)]
    end

    D1 -- "Taahhüt + Kanıt" --> P2P[GossipSub Mesh]
    D2 -- "Taahhüt + Kanıt" --> P2P
    D3 -- "Taahhüt + Kanıt" --> P2P

    P2P --> Registry
    Registry --> Buffer
    Buffer --> Verifier
    Verifier --> State
    State --> Storage

    Verifier -- "Eşdeğerlik Tespit Edildi" --> Freeze[Domaini Küresel Dondur]
    Freeze --> Registry
```

## 3. Mevcut Riskler ve Sınırlamalar

### Riskler
- **Erken Aşama Adapterlar:** Kesinlik kanıtı adapterları (PoS/BFT), tam kriptografik BLS/Ed25519 doğrulaması yerine şimdilik üst düzey imza eşiği mantığını kullanmaktadır.
- **Ağ Ölçeği:** Kontrollü bir harness içinde 5 düğümle test edilmiş olsa da, yüksek gecikmeli 100+ düğüm altındaki davranış henüz benchmark edilmemiştir.
- **Ekonomik Güvenlik:** Kesinleşmiş bir ekonomik slashing modelinin eksikliği, Bizans davranışı için şimdilik finansal bir caydırıcılık olmadığı, sadece protokol düzeyinde izolasyon sağlandığı anlamına gelir.

### Sınırlamalar
- **Üretime Hazır Değil:** Kod tabanı profesyonel güvenlik denetimleri gerektirir.
- **Resmi Doğrulama:** Konsensüs yakınsaması için TLA+ veya resmi kanıtlar bulunmamaktadır.
- **Kamuya Açık Testnet:** Şimdilik yerel devnet simülasyon harness'ları ile sınırlıdır.
- **Validator Ekonomisi:** Küresel katman için ödül dağıtımı ve validator seçimi henüz uygulanmamıştır.

## 4. Budlum Core v0.1 — Çoklu Konsensüs Yerleşim Prototipi
Deponun mevcut durumu **v0.1-settlement-prototype** sürümünü temsil etmektedir.

**Temel Başarılar:**
- [x] Heterojen domainler için deterministik küresel durum.
- [x] Bizans eşdeğerlik bağışıklığı (Model B).
- [x] Kalıcı sırasız yürütme tamponlaması (Out-of-order buffering).
- [x] Dağıtık düğüm yakınsaması doğrulandı.
