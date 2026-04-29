# Bölüm 3: Konsensüs Mekanizmaları

Bir blok zincirinin kalbi, konsensüs (fikir birliği) mekanizmasıdır. Dağıtık bir ağda, hangi bloğun geçerli olduğu ve zincirin hangi yöne gideceği konusunda tüm düğümlerin anlaşması gerekir.

Bu bölümde, Budlum projesinde desteklenen üç farklı konsensüs mekanizmasını inceleyeceğiz:

1.  **Proof of Work (PoW):** Bitcoin tarzı, hesaplama gücüne dayalı klasik madencilik.
2.  **Proof of Stake (PoS):** Modern, enerji verimli ve ekonomik teminatlara dayalı sistem.
3.  **Proof of Authority (PoA):** Özel ağlar için, belirli otoritelere güvenen sistem.

Budlum, bu mekanizmalar arasında geçiş yapabilecek modüler bir yapı (`ConsensusEngine` trait) üzerine kurulmuştur.

Ancak Budlum sadece mekanizma değiştirmekle kalmaz. Piyasada öncü bir **Multi-Consensus Settlement Mimarisi (Çoklu Konsensüs Uzlaşma Mimarisi)** sunar. Bu yapı sayesinde PoW, PoS ve PoA ağları tamamen izole alt ağlar (domainler) olarak aynı anda, tek bir ana uzlaşma (settlement) katmanı üzerinde çalışabilir. Bu bağımsız ağlar, kriptografik kanıtlarını bir Global Blok içerisinde birleştirerek, merkezi bir aracıya ihtiyaç duymadan birbirleri arasında otomatik ve güvenli varlık transferleri (Trustless Cross-Domain Bridge) yapabilirler.
