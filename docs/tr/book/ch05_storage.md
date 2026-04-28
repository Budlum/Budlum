# Bölüm 5: Depolama ve Verim

Bir blok zinciri sadece ağı ve konsensüsü değil, aynı zamanda verilerin nasıl saklandığını ve yönetildiğini de kapsar. Bu bölümde, Budlum'un veri katmanını inceleyeceğiz.

Veri katmanı üç ana bileşenden oluşur:

1.  **Kalıcı Depolama (Storage):** Blokların ve durumun (State) diske yazıldığı yer. (Sled DB)
2.  **Geçici Bellek (Mempool):** Henüz onaylanmamış işlemlerin RAM'de tutulduğu havuz.
3.  **Veri Budama (Pruning):** Disk alanından tasarruf etmek için eski verilerin silinmesi ve sadece özetlerin (Snapshot) saklanması.

Bu bileşenlerin uyumlu çalışması, düğümün performansını ve disk kullanımını doğrudan etkiler.
