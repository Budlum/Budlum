# Çoklu Konsensüs Yerleşim Katmanı (Multi-Consensus Settlement Layer)

Bu döküman, Budlum'un Çoklu Konsensüs Yerleşim Katmanı'nın (Model B) mimarisini, tasarım hedeflerini ve uygulama detaylarını özetler.

## 1. Problem
Geleneksel blokzincirleri tek bir konsensüs mekanizmasına (örneğin sadece PoW veya sadece PoS) bağlıdır. Ölçeklendirme genellikle varlıkları "köprüleyen" L2'ler veya yan zincirler içerir; bu da parçalanmış likidite ve güven gerektiren karmaşık yapılar oluşturur. Farklı konsensüs domainlerinin, güven gerektiren aracılar olmadan tek bir küresel durumu (global state) deterministik olarak güncellemesini sağlayan standart bir yapı yoktur.

## 2. Tasarım Hedefi
Budlum'un hedefi, aşağıdaki özelliklere sahip bir **Evrensel Yerleşim Katmanı** oluşturmaktır:
- Çoklu konsensüs domainlerinin (PoW, PoS, BFT, ZK) paralel çalışmasını desteklemek.
- Tek bir birleşik küresel hesap durumunu (unified global account state) zorunlu kılmak.
- Yerleşim seviyesinde Bizans Hata Toleransı (BFT) sağlamak.
- Taahhütleri kalıcı settlement state'e almadan önce metadata, finality, parent-link, validator-set ve nonce invaryantı kontrollerinden geçirmek.
- Taahhüt kabulü ile domain yükseklik/hash güncellemelerini atomik olarak kalıcılaştırmak; böylece yeniden başlatma sırasında yarım kalmış settlement geçişi görülmez.

## 3. Konsensüs Domain Modeli
Bir **Konsensüs Domaini**, kendi kurallarına sahip bağımsız bir blokzinciri veya yürütme ortamıdır.
- **Kimlik:** Her domainin benzersiz bir `DomainId`'si vardır.
- **Tür:** Konsensüs türünü (PoW, PoS vb.) tanımlar.
- **Operatör Kimliği:** Her kayıtlı domain sıfır olmayan bir operatör adresi ve minimum bond taşır. Operatör bond'u olmayan veya reserved zero operator kullanan kayıtlar reddedilir.
- **Registry:** Yerleşim Katmanı tüm aktif domainleri, mevcut yüksekliklerini, operatör bond'unu ve `ValidatorSetHash` değerlerini takip eder.
- **Adapterlar:** Her domain, durum geçişlerini Yerleşim Katmanı'na kanıtlamak için özel bir `FinalityAdapter` kullanır.

## 4. DomainCommitment Yapısı
`DomainCommitment`, bir domain tarafından yerleşim katmanına sunulan kriptografik kanıttır:
- `domain_id`: Güncellemenin kaynağı.
- `domain_height`: Taahhüt edilen bloğun yüksekliği.
- `state_root`: Domainin ortaya çıkan durumu.
- `state_updates`: Bu taahhütte güncellenen hesap nonce/bakiye haritası.
- `finality_proof_hash`: Konsensüse özel kanıta (örneğin PoW nonce veya PoS imzaları) referans.
- `parent_domain_block_hash`: Önceden commit edilmiş domain bloğunun hash'i. Production settlement parent-link uyuşmazlıklarını reddeder.
- `validator_set_hash`: Taahhüdü kayıtlı domain ve finality proof ile bağlayan validator-set ankrajı.

## 5. Yerleşim Katmanı (Settlement Layer)
Yerleşim Katmanı, Budlum ekosisteminin "Yüksek Mahkemesi" olarak görev yapar. İşlemleri yürütmez; **taahhütleri (commitments)** doğrular.
- Tüm doğrulanmış domain taahhütlerinin Merkle toplamı olan bir `GlobalBlockHeader` tutar.
- Domainlerin küresel kaydını (Global Registry) ve durumlarını (Aktif, Dondurulmuş, Emekli) yönetir.
- `GlobalBlockHeader` timestamp'i settlement header builder içinde deterministiktir; aynı state üzerinden tekrar build edilen header aynı hash'i üretir.

Ham domain commitment gönderimi public RPC ve production chain path'lerinde kapalıdır. Operatörler `VerifiedDomainCommitment` göndermelidir; içindeki proof hash commitment ile eşleşmeli ve ilgili adapter kayıtlı domain konfigürasyonuna göre finalized sonucu üretmelidir.

Adapter sertleştirmeleri:
- PoW hem yeterli confirmation depth hem de sıfır olmayan `total_work_hint` ister.
- PoS finality certificate'i validator snapshot'a karşı doğrular; snapshot/cert/commitment hash'lerini, varsa domainin kayıtlı `validator_set_hash` değerine bağlar.
- PoA/BFT/ZK adapterları mevcut quorum/proof-hash modellerini kullanır; BFT/ZK mismatch kontrolleri adapter seviyesinde uygulanır.

## 6. Küresel Paylaşımlı Durum Güvenliği (Global Shared-State Safety)
Çapraz domainler arası çift harcamayı (double-spending) önlemek için Budlum **Nonce İnvaryantı**'nı zorunlu kılar:
$$Account_{nonce}^{Global} < Commitment_{nonce}^{Domain}$$
Bir taahhüt, ancak nonce değeri o hesabın mevcut küresel nonce değerinden kesinlikle büyükse geçerlidir. Bu, iki domain aynı hesabı güncellemeye çalışsa bile belirli bir "Küresel Yükseklik"te yalnızca birinin başarılı olabilmesini sağlar.

Hemen uygulanabilir bir taahhütte geçersiz nonce claim'i varsa commitment registry'ye alınmadan reddedilir. Restart replay sırasında hesap nonce'ları yalnızca ileri taşınır, geriye sarılmaz.

## 7. Deterministik Çatışma Çözümü (Deterministic Conflict Resolution)
İki domain aynı hesap nonce'u için çakışan günceller gönderirse:
- Küresel yerleşim kaydına (P2P varış veya blok dahil edilme yoluyla) ilk ulaşan taahhüt kabul edilir.
- Aynı nonce için gelen sonraki tüm taahhütler settlement state'i etkilemeden reddedilir.

Aynı domain aynı yükseklik için iki farklı block hash gönderirse domain dondurulur. Birebir aynı duplicate commitment idempotent kabul edilir ve state'i değiştirmez.

## 8. Köprü Güvenliği
Cross-domain bridge işlemleri kayıtlı, aktif ve bridge-enabled domainlere bağlıdır:
- Asset registration kayıtlı bridge-enabled kaynak domain gerektirir.
- Lock işlemi birbirinden farklı, kayıtlı ve bridge-enabled source/target domainler, sıfır olmayan amount ve source event height sonrası expiry ister.
- Raw burn ve raw unlock path'leri kapalıdır. Fonların geri açılması için target-domain `BridgeBurned` event'i settlement'a commit edilmeli ve event Merkle proof ile doğrulanmalıdır.
- RPC istemcileri dönüş ayağı için `bud_burnBridgeTransferWithEvent` ve `bud_unlockBridgeTransferVerified` kullanmalıdır.

## 9. Gossip ve Ağ Yakınsaması (Gossip and Network Convergence)
Taahhütler, bir **Gossip Mesh** (`libp2p-gossipsub`) aracılığıyla yayılır.
- **Yakınsama (Convergence):** Honest (dürüst) düğümler sonunda aynı taahhüt setine ulaşır.
- **Idempotency:** Aynı taahhüdün tekrar sunulması durum üzerinde hiçbir etki yaratmaz.
- **Buffering:** Sırasız gelen taahhütler (örneğin 9. bloktan önce 10. bloğun gelmesi) bir `pending_buffer` içinde saklanır ve eksik parça tamamlandığında uygulanır.

## 10. Bizans Domain Yönetimi (Byzantine Domain Handling)
Bir domain kötü niyetli davranırsa (eşdeğerlik/equivocation):
- **Kanıt:** Çakışan taahhütler kayıt defterinde kanıt olarak saklanır.
- **Küresel Dondurma (Global Freeze):** Domainin durumu `Frozen` olarak değiştirilir. Bu domainden gelecek sonraki hiçbir taahhüt asla kabul edilmez.
- **Slashing Tetikleyicisi:** Dondurulmuş domainler, operatör bond modeli üzerinden ekonomik ceza için bağlayıcı sinyal üretir.

Validator seviyesindeki equivocation ayrı bir PoS slashing kanıtı akışıyla ele alınır:
- Double-sign tespit eden node `SlashingEvidence` üretir.
- Kanıt, üst seviye `NetworkMessage::SlashingEvidence` olarak gossip edilir.
- Blok üreticileri bekleyen kanıtları sonraki bloklara dahil eder; execution katmanı stake slashing'i uygular.

## 11. Kalıcılık ve Çökme Kurtarma (Persistence and Crash Recovery)
Katman, şu an `BlockchainStorage` trait'i arkasında çalışan kalıcı bir **Sled DB** kullanır. Değerler binary serialization ile yazılır; geçiş sürecinde eski JSON kayıtları okunmaya devam eder. Storage backend şu verileri saklar:
- Tüm domain taahhütleri (doğrulanmış ve bekleyen).
- Tüm domainlerin mevcut durumları.
- Küresel durum ağacı.
- Taahhüt insert + domain yükseklik/hash güncellemelerini kapsayan atomik settlement batch'leri.
- Düğüm yeniden başlatma mantığı, `pending_buffer` ve `Frozen` durumlarının anında geri yüklenmesini sağlayarak "yeniden başlatma sonrası eşdeğerlik" saldırılarını önler.

## 12. Mevcut Sınırlamalar
Mevcut repo kontrollü bir public devnet için uygundur; fakat audited mainnet dağıtımı değildir:
- **Denetim Bekliyor:** Profesyonel güvenlik denetimleri ve performans sertleştirmeleri hâlâ gereklidir.
- **Operasyonel Sertleştirme:** RPC rate limiting/auth, Docker/systemd paketleme, health check, fuzzing ve tam clippy temizliği hâlâ açıktır.
- **Error Refactor:** Yapılandırılmış `BudlumError` vardır ve kritik execution path'leri bunu kullanır; ancak bazı public API'ler geriye dönük uyumluluk için hâlâ `Result<T, String>` wrapper'ları sunar.
- **Formal Verification:** Matematiksel invaryantlar henüz TLA+ veya benzeri araçlarla resmi olarak doğrulanmamıştır.
- **Erken Aşama Adapterlar:** PoA/BFT adapterları hâlâ üst düzey quorum sayaçları kullanır; PoS artık certificate'i validator snapshot ve validator-set hash ankrajlarına karşı doğrular, ancak audit seviyesinde entegrasyon incelemesi gerekir.

## 13. Test Kapsamı
Katman, aşağıdakileri içeren bir **Bizans Kaos Matrisi** ile doğrulanmıştır:
- Ağ bölünmeleri (partition) ve uzlaşma.
- Domainler arası çift harcama koruması.
- Düğüm çökme/kurtarma döngüleri.
- Yüksek eşzamanlılık (concurrency) stres testleri.
