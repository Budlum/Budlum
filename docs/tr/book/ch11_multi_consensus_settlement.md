# Bölüm 11: Çoklu Konsensüs Yerleşim ve Bizans Dayanıklılığı

Bu bölüm, Budlum'un blokzinciri endüstrisinde eşi benzeri olmayan en büyük teknik devrimini ele almaktadır: **Dünyanın ilk heterojen, Bizans dayanıklılığına sahip Çoklu Konsensüs Yerleşim Katmanı.**

Bu kitapta "sıfırdan blokzinciri yazmayı" öğrendiğimiz için, bu bölümde sadece teoriyi değil, bu karmaşık yapının **matematiğini, kod uygulamasını ve sarsılmaz test metodolojisini** adım adım inceleyeceğiz.

## 1. Paradigma Değişimi: Monolitik ve Modüler Yapıların Ötesinde

Budlum'da farklı konsensüs domainleri (PoW, PoS vb.), settlement layer'ın eşzamanlı veri üreticileridir.

### Neden Dünyada Bir İlk?
1.  **Heterojen Konsensüs Co-existence:** Farklı kurallar aynı anda aynı Global Header içinde mühürlenir.
2.  **Shared Global Account State:** Varlıklar "wrap" edilmez, doğrudan güncellenir.
3.  **Forkless Reconciliation:** Sadece kesinleşmiş (finalized) kanıtlar kabul edilir.

### Bizans Kaos Matrisi (18 Senaryo)

Settlement layer, aşağıdaki kaos senaryolarının tamamında deterministik kalmayı başarır:

1.  **Gossip Convergence:** Farklı sıralarla gelen verilerin (gossip) sonunda tüm honest düğümlerde aynı `GlobalBlockHeader` hash'ine ulaşması.
2.  **Persistence Recovery:** Düğüm çökse bile tamponlanmış (pending) blokların veya "Frozen" domain statülerinin diskten eksiksiz geri yüklenmesi.
3.  **Adversarial Finality:** Yanlış PoS/PoW kanıtları veya yetersiz onay derinlikleri ile yapılan saldırıların reddedilmesi.
4.  **Atomic Recovery:** Commitment insert ve domain height update işlemlerinin yeniden başlatma sonrası tek kalıcı settlement geçişi olarak görülmesi.

## Faz 3: Dağıtık Devnet Simülasyonu (Distributed Test Harness)

Sistemin gerçek ağ koşullarındaki başarısı, `src/tests/distributed_settlement.rs` altında kurgulanan dağıtık test harness ile kanıtlanmıştır.

### Test Harness Mimarisi

*   **Mini-Network:** 5 adet tam teşekküllü `libp2p` düğümü.
*   **Isolated Storage:** Her düğüm için ayrı bir Sled veri tabanı dizini.
*   **Gossip Mesh:** Düğümler arası `gossipsub` protokolü ile commitment yayılımı.
*   **Chaos Engine:** Rastgele gecikmeler, sırasız paketler ve yapay düğüm çökmeleri (crash/restart).

### Kanıtlanan Özellikler

1.  **Idempotent Registry:** Aynı commitment'ın farklı düğümlerden veya tekrar tekrar gelmesi durumunda state bozulmaz.
2.  **Gap-Filling Persistence:** Diskten yüklenen "hole" (eksik blok) içeren veriler, eksik parçalar ağdan geldikçe otomatik olarak tamamlanır ve işletilir.
3.  **Global Invariant Verification:** Alice'in nonce'u tüm düğümlerde tam olarak aynı blok yüksekliğinde ve aynı değerle güncellenir.

---
*Budlum Core: Model B mimarisi ile blokzinciri yerleşim katmanı, artık sadece bir veritabanı değil, Bizans koşullarında çalışan deterministik bir state-machine'dir.*

## 2. Matematiksel Model: Nonce İnvaryantı ve Geçiş Fonksiyonu

Çoklu domain yapısında güvenliği sağlayan temel matematiksel kural **Nonce İnvaryantı**'dır. Bir Küresel Durum $G$ ve bir Domain Taahhüdü $C$ verildiğinde, durum geçiş fonksiyonu $f(G, C) \to G'$ şu kurala tabidir:

$$Account_{nonce}(G') = 
\begin{cases} 
C_{nonce}, & \text{eğer } C_{nonce} > Account_{nonce}(G) \\
Account_{nonce}(G), & \text{aksi halde}
\end{cases}$$

Bu formül, **"Sadece ileriye dönük ve daha büyük nonce"** kuralını işletir. Bu, **Double Spend**'i matematiksel olarak imkansız kılar.

## 3. Pratik Uygulama: Settlement Motorunu Kodlamak

### Adım 1: Taahhüt Kabulü, Equivocation Kontrolü ve Atomik Kalıcılık

```rust
pub fn submit_domain_commitment(&mut self, commitment: DomainCommitment) -> Result<(), String> {
    self.validate_domain_commitment_metadata(&commitment)?;
    
    if let Some(existing) = self.domain_commitment_registry.find_by_height(
        commitment.domain_id,
        commitment.domain_height,
    ) {
        if existing.domain_block_hash != commitment.domain_block_hash {
            // AYNI yükseklik, FARKLI hash -> DOMAİNİ DONDUR!
            let d_mut = self.domain_registry.get_mut(commitment.domain_id).unwrap();
            d_mut.status = DomainStatus::Frozen;
            return Err("Equivocation detected! Domain frozen.".into());
        }
        return Ok(());
    }

    self.domain_commitment_registry.insert(commitment.clone())?;
    let updated_domains = self.apply_pending_commitments(commitment.domain_id)?;

    if let Some(store) = &self.storage {
        store.save_domain_commitment_batch(&commitment, &updated_domains)
            .map_err(|e| format!("Failed to persist settlement batch: {}", e))?;
    }

    Ok(())
}
```

### Adım 2: Asenkron Tamponlama (Apply Loop)

```rust
fn apply_pending_commitments(&mut self, domain_id: DomainId) -> Result<Vec<ConsensusDomain>, String> {
    let mut updated_domains = Vec::new();

    loop {
        let last_height = self.domain_registry.get(domain_id).unwrap().last_committed_height;
        let next_height = last_height + 1;
        
        if let Some(com) = self.domain_commitment_registry.find_by_height(domain_id, next_height) {
            let mut can_apply = true;
            for (addr, new_nonce) in &com.state_updates {
                if *new_nonce <= self.state.get_nonce(addr) {
                    can_apply = false; 
                    break;
                }
            }
            // ... state uygulama mantığı ...
            updated_domains.push(self.domain_registry.get(domain_id).unwrap().clone());
        } else {
            break; 
        }
    }

    Ok(updated_domains)
}
```

Buradaki kritik production-hardening noktası, commitment kaydı ile domain yükseklik/hash güncellemesinin tek storage batch içinde yazılmasıdır. Node yeniden başlatıldığında "commitment var ama height ilerlememiş" gibi yarım kalıcı durum görülmemelidir.

### Adım 3: Domain Operatörleri ve Slashing Evidence Gossip

Domain registration artık operatör adresi ve minimum bond taşır; bu da frozen domain'ler için ekonomik ceza bağlantısını kurar. Validator seviyesinde double-sign ise ayrı akar: PoS motoru `SlashingEvidence` üretir, node bunu `NetworkMessage::SlashingEvidence` olarak gossip eder, blok üreticileri bekleyen kanıtları bloğa koyar ve execution katmanı stake slashing'i deterministik uygular.

## 4. Bizans Kaos Matrisi: Gerçeği İspatlamak

Sıfırdan blokzinciri yazarken en kritik aşama, kodunuzu "kaos" altında test etmektir. Budlum Settlement Layer, **18 senaryoluk bir Bizans Kaos Matrisi** ile test edilir. Bu testlerin her biri, sistemin bir Bizans (hatalı/kötü niyetli) ağda nasıl ayakta kaldığını kanıtlar.

### Kategori 1: Konverjans ve Sıralama Bağımsızlığı (Convergence)
Gerçek dünyada ağ paketleri sırasız gelir. 
*   **Test Mantığı:** Node A'ya paketleri 1-2-3 sırasında, Node B'ye ise 3-2-1 sırasında veririz.
*   **İspat:** Her iki node'un sonunda ürettiği `GlobalBlockHeader` hash'i bit düzeyinde aynı olmalıdır.

### Kategori 2: Ağ Bölünmesi ve Tamponlama (Partition & Buffering)
*   **Senaryo:** Ağ ikiye bölünür. Node A sadece PoW kanıtlarını, Node B sadece PoS kanıtlarını görür.
*   **İspat:** Ağ tekrar birleştiğinde (Gossip senkronizasyonu), her iki düğüm de registry'lerindeki eksikleri tamamlayıp aynı küresel duruma ulaşır.

### Kategori 3: İki Yüzlülük (Equivocation) Koruması
*   **Senaryo:** Kötü niyetli bir domain, aynı yükseklik için iki farklı blok hash'i yayar.
*   **İspat:** Sistem bunu anında algılar, ikinci commitment'ı reddeder ve domain'i sonsuza kadar "Frozen" durumuna alarak ana settlement root'undan çıkartır.

### Kategori 4: Eşzamanlılık ve Yarış Koşulları (Concurrency)
*   **Senaryo:** 100 farklı Tokio task'ı aynı anda `submit_domain_commitment` çağrısı yapar.
*   **İspat:** `RwLock` ve atomik operasyonlar sayesinde, hiçbir yarış koşulu (race condition) oluşmaz ve durum bütünlüğü korunur.

### Örnek Test Kodu: Sıralama Bağımsızlığı

```rust
#[tokio::test]
async fn test_order_independence() {
    let mut node_a = make_node();
    let mut node_b = make_node();
    let commitments = make_sample_commitments(100);

    // Node A: Normal sıra
    for com in &commitments { node_a.submit(com).unwrap(); }

    // Node B: Ters sıra
    for com in commitments.iter().rev() { node_b.submit(com).unwrap(); }

    assert_eq!(node_a.global_root(), node_b.global_root());
}
```

## 5. Sonuç

Budlum'un Çoklu Konsensüs Yerleşim Katmanı artık kontrollü public devnet adayıdır: Bizans settlement testlerinde deterministiktir, devnet seviyesinde slashing/reward ekonomisine bağlanmıştır ve mainnet öncesinde hâlâ audit, operasyonel sertleştirme ve formal verification beklemektedir.
