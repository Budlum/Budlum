# Bölüm 6: JSON-RPC 2.0 API

Budlum düğümü, dış dünyayla (cüzdanlar, explorer'lar, araçlar) konuşmak için **JSON-RPC 2.0** standartlarını kullanır. Bu arayüz `jsonrpsee` kütüphanesi üzerine inşa edilmiştir.

Kaynaklar:
- `src/rpc/api.rs` (Tanımlar)
- `src/rpc/server.rs` (Uygulama)

## 1. Çalıştırma

```bash
cargo run -- --rpc-host 0.0.0.0 --rpc-port 9999
```

### Konfigürasyon Dosyası (TOML)

Komut satırı argümanları yerine `budlum.toml` dosyası kullanılarak daha karmaşık ayarlar yapılabilir:

```bash
cargo run -- --config ./budlum.toml
```

**Örnek `budlum.toml`:**
```toml
[network]
name = "testnet"
chain_id = 42
port = 5001

[bootnodes]
addresses = ["/ip4/203.0.113.10/tcp/5001/p2p/12D3K..."]

[rpc]
enabled = true
host = "127.0.0.1"
port = 8545
auth_required = true
api_key_env = "BUDLUM_RPC_API_KEY"
rate_limit_per_minute = 600

[metrics]
port = 9090

[storage]
db_path = "./data/testnet/budlum.db"
```

Hazır profiller repo kökünde bulunur: `config/mainnet.toml`, `config/testnet.toml`, `config/devnet.toml`.

---

## 2. Gözlemlenebilirlik: Prometheus Metrikleri

Düğümün sağlığını ve performansını izlemek için `/metrics` endpoint'i üzerinden gerçek zamanlı veriler sunulur.

- **Varsayılan Port:** `9090`
- **Erişim:** `http://127.0.0.1:9090/metrics`

**Sunulan Metrikler:**
- `budlum_chain_height`: Güncel blok yüksekliği.
- `budlum_peer_count`: Bağlı eş (peer) sayısı.
- `budlum_mempool_size`: Havuzdaki bekleyen işlem sayısı.
- `budlum_reorgs_total`: Gerçekleşen toplam reorg sayısı.
- `budlum_finalized_height`: En son finalize edilmiş blok.
- `budlum_block_propagation_seconds`: Blok yayılım süresi histogramı.
- `budlum_mempool_sender_count`: Mempool'daki farklı gönderici sayısı.
- `budlum_peer_connection_quality`: Peer bağlantı kalitesi skoru.
- `budlum_consensus_round_seconds`: Konsensüs tur süresi histogramı.
- `budlum_finality_lag`: Head yüksekliği ile finalized height arasındaki fark.

---

## 3. Desteklenen Metotlar (`bud_` Prefixi)

Tüm metotlar `bud_` ön eki ile başlar. Bu, ağa özgü metotları standart olanlardan ayırmamızı sağlar.

| Metot | Parametreler | Açıklama |
| :--- | :--- | :--- |
| `bud_chainId` | `[]` | Ağın Chain ID'sini döner (örn: 1337). |
| `bud_blockNumber` | `[]` | En son bloğun yüksekliğini döner. |
| `bud_getBlockByNumber`| `[id: u64]` | Belirtilen numaradaki blok verisini döner. |
| `bud_getBlockByHash` | `[hash: string]` | Belirtilen hash'e sahip bloğu döner. |
| `bud_getBalance` | `[addr: string]`| Verilen adresin bakiyesini döner. |
| `bud_getNonce` | `[addr: string]`| Adresin işlem sayısını (nonce) döner. |
| `bud_sendRawTransaction`| `[tx: object]` | İmzalanmış işlemi ağa gönderir. |
| `bud_getTransactionByHash`| `[hash: string]`| İşlem detaylarını döner. (O(1) İndeksli) |
| `bud_getTransactionReceipt`| `[hash: string]`| İşlemin işlenme sonucunu (fişini) döner. (O(1) İndeksli) |
| `bud_gasPrice` | `[]` | Ağdaki güncel `base_fee` değerini döner. |
| `bud_estimateGas` | `[tx: object]` | Tahmini gas tüketimini döner. |
| `bud_txPrecheck` | `[tx: object]` | İşlemi mempool ve chain bağlamında önceden simüle eder. |
| `bud_syncing` | `[]` | Düğümün senkronizasyon durumunu döner. |
| `bud_netVersion` | `[]` | Ağ versiyonunu (Network ID) döner. |
| `bud_netListening` | `[]` | Düğümün dinleme durumunu döner. |
| `bud_netPeerCount` | `[]` | Bağlı eş sayısını döner. |

## 3. Örnek Kullanım (curl)

**Blok Sayısını Sorgulama:**
```bash
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"bud_blockNumber","params":[],"id":1}' \
  http://127.0.0.1:8545
```

**Bakiye Sorgulama:**
```bash
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"bud_getBalance","params":["BURADA_ADRES"],"id":1}' \
  http://127.0.0.1:8545
```

## 4. Mimari Tasarım ve Güvenlik (Hardening)

RPC sunucusu, asenkron bir `tokio` görevinde çalışır. **Mainnet Ready** aşamasında aşağıdaki güvenlik katmanları eklenmiştir:

1. **Bağlantı Sınırı (Max Connections):** Aynı anda en fazla 100 aktif bağlantıya izin verilir. Bu, kaynak tükenmesini (resource exhaustion) önler.
2. **Payload Sınırı (Max Request Size):** Gelen her RPC isteği en fazla **2 MB** olabilir. Çok büyük JSON paketleri ile belleği şişirme saldırıları bu sayede engelenir.
3. **İşlem Doğrulama (TX Validation):** `bud_sendRawTransaction` metodu, işlemi ağa yaymadan önce **transaction size** (Max 100KB) ve **kriptografik imza** kontrolü yapar. Hatalı veya devasa işlemler anında reddedilir.
4. **Panic Prevention:** Sunucu kodundaki tüm kritik noktalar `Result` tipiyle yönetilir. Bozuk bir JSON veya ağ hatası tüm düğümü çökertemez.
5. **Config Tabanlı Auth Hazırlığı:** TOML dosyalarında `auth_required`, `api_key_env`, `allowed_ips`, `cors_origins` ve `rate_limit_per_minute` alanları bulunur. Bunlar prod operatör konfigürasyonunu standartlaştırır; enforcement katmanı ayrıca genişletilmelidir.

## 5. `bud_txPrecheck` Ne Kadar Gerçekçi?

Budlum'un güncel `bud_txPrecheck` implementasyonu artık sadece kaba bir "imza ve bakiye" kontrolü değildir. İstek, doğrudan `ChainActor` üzerinden zincirin gerçek state'ine ve mempool bağlamına sorulur.

Bu metot aşağıdaki durumları raporlayabilir:
- `invalid_signature`
- `invalid_chain_id`
- `fee_too_low`
- `nonce_too_low`
- `nonce_too_high`
- `insufficient_funds`
- `missing_to_address`
- `invalid_stake_amount`
- `not_a_validator`
- `insufficient_stake`
- `duplicate_transaction`
- `rbf_fee_too_low`
- `sender_limit_reached`
- `pool_full`

Önemli nokta şudur: aynı göndericiden mempool'da zaten bekleyen ardışık işlemler varsa, precheck bunları da hesaba katar. Yani cüzdan tarafında "bir sonraki nonce ne olmalı?" sorusuna daha gerçekçi cevap verir.
