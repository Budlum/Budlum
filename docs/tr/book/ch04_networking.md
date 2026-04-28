# Bölüm 4: Ağ ve P2P İletişimi

Blok zinciri, doğası gereği dağıtık (distributed) bir sistemdir. Merkezi bir sunucusu yoktur. Bunun yerine, dünyanın dört bir yanına dağılmış binlerce bilgisayar (düğüm/node) birbirine bağlanarak ağı oluşturur.

Bu bölümde, Budlum düğümlerinin:
1.  Birbirlerini nasıl bulduklarını (Discovery),
2.  Nasıl iletişim kurduklarını (Protocol),
3.  Kötü niyetli aktörleri nasıl elediklerini (Peer Management) inceleyeceğiz.

Altyapı olarak endüstri standardı **libp2p** kütüphanesini kullanıyoruz.
