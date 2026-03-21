# Bölüm 7: Ağ Ayrımı (Network Distinctions)

Budlum ağları, kullanım amaçlarına göre farklı parametrelerle çalışır. `mainnet`, `testnet` ve `devnet` olmak üzere üç ana ağ tipi desteklenir.

## 1. Ağ Tipleri

| Ağ | Amacı | Varsayılan Port | Chain ID | Bootnode'lar |
| :--- | :--- | :--- | :--- | :--- |
| **Mainnet** | Canlı ağ, gerçek değer üretimi. | 30303 | 1 | Sabit listelenmiş güvenilir node'lar. |
| **Testnet** | Geliştiriciler için ücretsiz test ortamı. | 30304 | 2 | Topluluk tarafından sağlanan node'lar. |
| **Devnet** | Yerel geliştirme ve Chaos testleri. | 5001 | 42 | Yok (genellikle manuel bağlanılır). |

## 2. CLI ile Ağ Seçimi

Düğümü başlatırken `--network` bayrağı kullanılır:

```bash
# Devnet modunda başlat (Varsayılan)
cargo run -- --network devnet

# Testnet modunda başlat
cargo run -- --network testnet
```

## 3. Yapılandırma Mantığı (`src/core/chain_config.rs`)

Her ağın yapılandırması `ChainConfig` struct'ı içinde saklanır. Bu yapı şunları içerir:
- **Chain ID**: EIP-155 tarzı replay protection için.
- **Port**: P2P iletişimi için dinlenecek port.
- **Bootnodes**: Ağa ilk girişte bağlanılacak adresler.
- **Genesis State**: Ağın başlangıç bakiyeleri ve validatörleri.

## 4. Güvenlik ve İzolasyon

- Farklı ağlar arasında **Handshake** seviyesinde kontrol yapılır. Yanlış ağdaki bir node, P2P bağlantısını kabul etmez.
- `ChainID` sayesinde, bir ağda imzalanmış işlem diğer ağda geçersiz sayılır (Replay Protection).
