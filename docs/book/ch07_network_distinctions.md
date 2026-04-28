# Bölüm 7: Ağ Ayrımı (Network Distinctions)

Budlum ağları, kullanım amaçlarına göre farklı parametrelerle çalışır. `mainnet`, `testnet` ve `devnet` olmak üzere üç ana ağ tipi desteklenir.

## 1. Ağ Tipleri

| Ağ | Amacı | Varsayılan Port | Chain ID | Bootnode'lar |
| :--- | :--- | :--- | :--- | :--- |
| **Mainnet** | Canlı ağ, gerçek değer üretimi. | 4001 | 1 | Gerçek operatör multiaddr'leri zorunlu. |
| **Testnet** | Geliştiriciler için ücretsiz test ortamı. | 5001 | 42 | Testnet operatör multiaddr'leri. |
| **Devnet** | Yerel geliştirme ve Chaos testleri. | 6001 | 1337 | Yok (genellikle manuel bağlanılır). |

## 2. CLI ile Ağ Seçimi

Düğümü başlatırken `--network` bayrağı kullanılır:

```bash
# Devnet modunda başlat (Varsayılan)
cargo run -- --network devnet

# Testnet modunda başlat
cargo run -- --network testnet

# TOML config ile başlat
cargo run -- --config config/testnet.toml
```

## 3. Yapılandırma Mantığı (`src/core/chain_config.rs`)

Her ağın yapılandırması `Network` metotları ve `config/*.toml` dosyalarıyla yönetilir. Bu yapı şunları içerir:
- **Chain ID**: EIP-155 tarzı replay protection için.
- **Port**: P2P iletişimi için dinlenecek port.
- **Bootnodes / Fallback Bootnodes / DNS Seeds**: Ağa ilk girişte bağlanılacak adresler. Mainnet'te placeholder yoktur; gerçek adres girilmeden node başlamaz.
- **Genesis State**: Ağın başlangıç bakiyeleri ve validatörleri.
- **Consensus Params**: `epoch_len`, `min_stake`, finality quorum ve slot süresi.
- **Gas Schedule**: Base fee, byte başına gas, imza gas maliyeti ve işlem tipi maliyetleri.
- **Mempool Config**: Maksimum havuz boyutu, gönderici limiti, minimum fee, TTL ve RBF bump oranı.
- **Security Config**: Maksimum peer sayısı, RPC auth beklentisi, rate-limit hedefleri, ban persistence ve mDNS politikası.

## 4. Config Dosyaları

Repo kökünde üç hazır config bulunur:

```text
config/
├── mainnet.toml
├── testnet.toml
└── devnet.toml
```

`mainnet.toml` bilinçli olarak boş bootnode listeleriyle gelir. Canlı ağ öncesi `[bootnodes].addresses` gerçek libp2p multiaddr değerleriyle doldurulmalıdır:

```toml
[network]
name = "mainnet"
chain_id = 1
port = 4001

[consensus]
type = "pos"
min_stake = 1000000
epoch_len = 100

[bootnodes]
addresses = [
  "/ip4/203.0.113.10/tcp/4001/p2p/12D3K...",
]
```

## 5. Güvenlik ve İzolasyon

- Farklı ağlar arasında **Handshake** seviyesinde kontrol yapılır. Yanlış ağdaki bir node, P2P bağlantısını kabul etmez.
- Handshake sırasında protocol major/minor uyumluluğu kontrol edilir. Uyumsuz peer banlanır.
- `ChainID` sayesinde, bir ağda imzalanmış işlem diğer ağda geçersiz sayılır (Replay Protection).
- `Network::magic_bytes()` her ağ için ayrı magic byte tanımlar; binary protokol izolasyonu için kullanılacak sabit kaynak burasıdır.
