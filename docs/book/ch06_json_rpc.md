# Bölüm 6: JSON-RPC 2.0 API

Budlum düğümü, dış dünyayla (cüzdanlar, explorer'lar, araçlar) konuşmak için **JSON-RPC 2.0** standartlarını kullanır. Bu arayüz `jsonrpsee` kütüphanesi üzerine inşa edilmiştir.

Kaynaklar:
- `src/rpc/api.rs` (Tanımlar)
- `src/rpc/server.rs` (Uygulama)

## 1. Çalıştırma

RPC sunucusu varsayılan olarak `127.0.0.1:8545` adresinde dinler. CLI üzerinden değiştirilebilir:

```bash
cargo run -- --rpc-host 0.0.0.0 --rpc-port 9999
```

## 2. Desteklenen Metotlar (`bud_` Prefixi)

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
| `bud_getTransactionByHash`| `[hash: string]`| İşlem detaylarını döner. |
| `bud_getTransactionReceipt`| `[hash: string]`| İşlemin işlenme sonucunu (fişini) döner. |
| `bud_gasPrice` | `[]` | Önerilen işlem ücretini döner. |
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
