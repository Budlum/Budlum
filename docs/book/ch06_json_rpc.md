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
db_path = "./data/mainnet.db"
rpc_host = "127.0.0.1"
rpc_port = 8545
metrics_port = 9090
bootstrap = "/ip4/1.2.3.4/tcp/4001/p2p/..."
```

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

## 4. Mimari Tasarım

RPC sunucusu, asenkron bir `tokio` görevinde çalışır. `Blockchain` verisine erişmek için `Arc<Mutex<Blockchain>>` kullanır. Ağ verileri (peer count vb.) için ise `NodeClient` üzerinden ağ döngüsüyle iletişim kurar.
