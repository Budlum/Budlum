# Bölüm 3: Konsensüs Mekanizmaları

Bir blok zincirinin kalbi, konsensüs (fikir birliği) mekanizmasıdır. Dağıtık bir ağda, hangi bloğun geçerli olduğu ve zincirin hangi yöne gideceği konusunda tüm düğümlerin anlaşması gerekir.

Bu bölümde, Budlum projesinde desteklenen üç farklı konsensüs mekanizmasını inceleyeceğiz:

1.  **Proof of Work (PoW):** Bitcoin tarzı, hesaplama gücüne dayalı klasik madencilik.
2.  **Proof of Stake (PoS):** Modern, enerji verimli ve ekonomik teminatlara dayalı sistem.
3.  **Proof of Authority (PoA):** Özel ağlar için, belirli otoritelere güvenen sistem.

Budlum, bu mekanizmalar arasında geçiş yapabilecek modüler bir yapı (`ConsensusEngine` trait) üzerine kurulmuştur.
