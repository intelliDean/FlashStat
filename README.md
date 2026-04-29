# 🏮 FlashStat
**The Transparency Layer for Unichain Soft-Finality**

FlashStat provides real-time cryptographic confidence scores for Unichain's 200ms Flashblocks. It monitors the sequencer for equivocations (soft-reorgs) and provides an Ethereum-compatible JSON-RPC interface for wallets and DApps.

## 🏗 Architecture
FlashStat is built as a high-performance Rust monorepo:

- **`bin/flashstat`**: The primary indexing engine. Subscribes to 200ms Flashblocks via WebSockets.
- **`bin/flashstat-server`**: JSON-RPC server providing confidence metrics.
- **`crates/flashstat-core`**: Core monitoring and reorg detection logic.
- **`crates/flashstat-db`**: Ultra-low latency persistence layer using redb (pure-Rust).
- **`crates/flashstat-api`**: Type-safe JSON-RPC interface definitions.

## 🚀 Getting Started

### Prerequisites
- Rust (Latest Stable)

### Configuration
Edit `flashstat.toml` or set environment variables:
```toml
[rpc]
ws_url = "wss://sepolia.unichain.org"
http_url = "https://sepolia.unichain.org"

[storage]
db_path = "./data/flashstat_db"
```

### Running the Monitor
```bash
cargo run -p flashstat
```

### Running the API Server
```bash
cargo run -p flashstat-server
```

## 📡 JSON-RPC API
The API server runs by default on `127.0.0.1:9944`.

### `flash_getConfidence`
Returns the cryptographic confidence score for a given block hash.
```bash
curl -X POST -H "Content-Type: application/json" --data '{"jsonrpc":"2.0","method":"flash_getConfidence","params":["0x..."],"id":1}' http://localhost:9944
```

## 🛡 Security & Trust
FlashStat calculates confidence based on:
1. **Persistence**: Number of consecutive sub-blocks seen for a hash.
2. **TEE Validity**: Verification of the Intel TDX sequencer signature (In Progress).
3. **Equivocation Checks**: Detection of conflicting TEE signatures for the same slot.

---
Built with 🦀 by One Block Org.
