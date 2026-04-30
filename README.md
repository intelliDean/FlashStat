# 🏮 FlashStat
**The Transparency Layer for Unichain Soft-Finality**

FlashStat provides real-time cryptographic confidence scores for Unichain's 200ms Flashblocks. It monitors the sequencer for equivocations (soft-reorgs) and provides an Ethereum-compatible JSON-RPC interface with active fraud proof protection (Watchtower).

## 🏗 Architecture
FlashStat is built as a high-performance Rust monorepo:

- **`bin/flashstat-server`**: Main entry point. Runs the JSON-RPC server and the indexing engine.
- **`bin/flashstat-tui`**: Terminal UI Dashboard for real-time monitoring and forensics.
- **`bin/flashstat-simulate`**: Forensic simulation tool for testing detection and slashing.
- **`crates/flashstat-core`**: Core monitoring, TEE verification, and reorg detection logic.
- **`crates/flashstat-db`**: Ultra-low latency persistence layer using `redb`.
- **`crates/flashstat-api`**: Type-safe JSON-RPC interface definitions.

## 🚀 Getting Started

### Prerequisites
- Rust (Latest Stable)

### Configuration
Edit `flashstat.toml` or set environment variables:
```toml
[rpc]
ws_url = "wss://unichain-sepolia..."
http_url = "https://unichain-sepolia..."

[guardian]
private_key = "0x..." # Or set FLASHSTAT__GUARDIAN__PRIVATE_KEY
slashing_contract = "0x..."
```

### Running the System
1. **Start the Monitor & Server**:
   ```bash
   cargo run -p flashstat-server
   ```

2. **Launch the Dashboard**:
   ```bash
   cargo run -p flashstat-tui
   ```

## 📡 JSON-RPC API
The API server runs by default on `127.0.0.1:9944`.

### Key Methods
- `flash_getConfidence`: Returns confidence score for a hash.
- `flash_getLatestBlock`: Returns the most recent processed block.
- `flash_getSequencerRankings`: Returns reputation stats for all sequencers.
- `flash_ingestBlock`: Manually submit a block for analysis (useful for external indexers).

## 🛡 Security & Active Protection
FlashStat doesn't just watch; it protects.
- **TEE Attestation**: Verifies Intel TDX quotes for every sequencer signature.
- **Reputation Scoring**: Tracks sequencer performance and reset streaks on reorgs.
- **Active Watchtower**: Automatically submits fraud proofs to the `SlashingManager` contract upon detecting equivocation.

---
Built with 🦀 by One Block Org.
