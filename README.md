# FlashStat

**FlashStat** is a real-time Unichain sequencer monitoring and fraud detection system. It watches the OP-Stack sequencer for misbehaviour — soft reorgs, equivocations, and double-spend attempts — and can autonomously submit on-chain slashing proofs via a Guardian Wallet when a violation is detected.

The system combines cryptographic TEE (Intel TDX) attestation verification with a persistent block-confidence model and a live reputation scoring engine for sequencer addresses, surfaced through a JSON-RPC API and a terminal dashboard.

---

## Table of Contents

- [Features](#features)
- [Architecture](#architecture)
- [Workspace Layout](#workspace-layout)
- [Prerequisites](#prerequisites)
- [Configuration](#configuration)
  - [File-based configuration](#file-based-configuration)
  - [Environment variable overrides](#environment-variable-overrides)
  - [Full reference](#full-reference)
- [Running the System](#running-the-system)
  - [1. Core Monitor (`flashstat`)](#1-core-monitor-flashstat)
  - [2. RPC Server (`flashstat-server`)](#2-rpc-server-flashstat-server)
  - [3. Terminal Dashboard (`flashstat-tui`)](#3-terminal-dashboard-flashstat-tui)
  - [4. Forensic Simulator (`flashstat-simulate`)](#4-forensic-simulator-flashstat-simulate)
- [Guardian Wallet (Active Protection)](#guardian-wallet-active-protection)
  - [Private-key mode](#private-key-mode)
  - [Keystore mode](#keystore-mode)
  - [Slashing contract ABI](#slashing-contract-abi)
- [JSON-RPC API Reference](#json-rpc-api-reference)
  - [Methods](#methods)
  - [Subscriptions](#subscriptions)
- [Confidence Model](#confidence-model)
- [Reputation Scoring](#reputation-scoring)
- [TEE / TDX Attestation](#tee--tdx-attestation)
- [Proof Encoding](#proof-encoding)
- [Crate Reference](#crate-reference)
- [Testing](#testing)
- [CI](#ci)
- [Development Notes](#development-notes)

---

## Features

| Capability | Detail |
|---|---|
| **Real-time block monitoring** | WebSocket subscription to Unichain with HTTP polling fallback |
| **Soft reorg detection** | Flags same-height block hash conflicts immediately |
| **Equivocation detection** | Identifies same-height, same-signer, different-hash conflicts |
| **Conflict analysis** | Diffs transactions across conflicting blocks; identifies dropped txs and double-spends |
| **TEE signature verification** | Recovers the ECDSA signer from OP-Stack `extra_data` and verifies against the configured sequencer |
| **Intel TDX attestation** | Optional structural validation of TDX Quote V4 and MRENCLAVE comparison |
| **Block confidence scoring** | Combines persistence depth and TEE verification into a `0–99%` confidence score |
| **Reputation engine** | Tracks per-sequencer blocks signed, streaks, attestations, reorgs, and equivocations |
| **Active slashing** | Guardian Wallet submits RLP-encoded on-chain equivocation proofs to a SlashingManager contract |
| **JSON-RPC server** | Full request/response API plus WebSocket pub/sub for live block and reorg streams |
| **Terminal UI** | Live dashboard with block feed, confidence bar, sequencer leaderboard, and reorg log |
| **Persistent storage** | Embedded [redb](https://github.com/cberner/redb) key-value database — no external dependencies |

---

## Architecture

```
                        ┌─────────────────────────────────────────┐
                        │            Unichain (OP-Stack)           │
                        │         WebSocket / HTTP RPC             │
                        └─────────────────┬───────────────────────┘
                                          │ blocks
                                          ▼
                              ┌───────────────────────┐
                              │     FlashMonitor        │
                              │  (flashstat-core)       │
                              │                         │
                              │  verify_tee_signature() │
                              │  classify_reorg()       │
                              │  update_reputation()    │
                              │  spawn_watchtower_task()│
                              └─────────┬───────────────┘
                                        │
                    ┌───────────────────┼────────────────────────┐
                    ▼                   ▼                         ▼
          ┌──────────────┐   ┌──────────────────┐   ┌────────────────────┐
          │  RedbStorage  │   │  block_tx channel │   │  event_tx channel  │
          │ (flashstat-db)│   │  (FlashBlock)     │   │  (ReorgEvent)      │
          └──────────────┘   └────────┬─────────┘   └─────────┬──────────┘
                                      │                         │
                            ┌─────────┴─────────────────────────┴──────────┐
                            │                FlashServer                     │
                            │           (flashstat-server)                   │
                            │                                                │
                            │  JSON-RPC / WebSocket on :9944                 │
                            └────────────────────────────────────────────────┘
                                             │
                         ┌───────────────────┼───────────────────┐
                         ▼                   ▼                   ▼
              ┌──────────────────┐  ┌──────────────┐  ┌──────────────────┐
              │  flashstat-tui   │  │ External RPC  │  │  Guardian Wallet │
              │  (Terminal UI)   │  │   Clients     │  │  → SlashingMgr   │
              └──────────────────┘  └──────────────┘  └──────────────────┘
```

---

## Workspace Layout

```
FlashStat/
├── bin/
│   ├── flashstat/                   # Core monitor entry point
│   ├── flashstat-server/            # JSON-RPC + WebSocket server
│   ├── flashstat-tui/               # Terminal dashboard
│   ├── flashstat-simulate/          # Forensic simulation tool
│   └── flashstat-watchtower-test/   # Integration test harness (+ manual smoke-test binary)
│
└── crates/
    ├── flashstat-common/       # Config, shared types, data models
    ├── flashstat-core/         # Monitor logic, TEE, wallet, proof encoding
    │   ├── src/monitor.rs      # FlashMonitor + all helper functions
    │   ├── src/tee.rs          # TeeVerifier (ECDSA recovery + TDX V4)
    │   ├── src/wallet.rs       # GuardianWallet (on-chain slashing)
    │   └── src/proof.rs        # RLP proof encoders
    ├── flashstat-db/           # RedbStorage persistence layer
    └── flashstat-api/          # jsonrpsee #[rpc] trait definition
```

---

## Prerequisites

| Requirement | Version |
|---|---|
| Rust | stable (edition 2024) |
| Unichain (or OP-Stack) RPC node | Any node with `eth_subscribe` support |
| Guardian Wallet | Optional — Ethereum private key or ERC-55 keystore |
| Slashing contract | Optional — deployed `SlashingManager` contract address |

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Build all workspace binaries:

```bash
cargo build --release
```

---

## Configuration

FlashStat uses the [`config`](https://docs.rs/config) crate. It loads from:

1. `flashstat.toml` (optional, in the current working directory)
2. Environment variables prefixed `FLASHSTAT__` with `__` as the separator

Both sources are merged, with environment variables taking precedence.

### File-based configuration

Create `flashstat.toml` in the directory from which you run the binaries:

```toml
[rpc]
ws_url   = "wss://unichain-sepolia.rpc.example.com"
http_url = "https://unichain-sepolia.rpc.example.com"

[storage]
db_path = "./flashstat.db"

[tee]
sequencer_address   = "0xYourSequencerAddress"
attestation_enabled = false
# expected_mrenclave = "deadbeef..."  # optional: hex MRENCLAVE for TDX pinning

[guardian]
# Choose ONE of private_key or keystore_path:
# private_key    = "0xYourPrivateKey"
# keystore_path  = "/path/to/keystore.json"
slashing_contract = "0xYourSlashingManagerAddress"
```

### Environment variable overrides

Every config key maps to `FLASHSTAT__<SECTION>__<KEY>` (all uppercase, double-underscore separator):

```bash
export FLASHSTAT__RPC__WS_URL="wss://unichain-sepolia.rpc.example.com"
export FLASHSTAT__RPC__HTTP_URL="https://unichain-sepolia.rpc.example.com"
export FLASHSTAT__STORAGE__DB_PATH="./flashstat.db"
export FLASHSTAT__TEE__SEQUENCER_ADDRESS="0xYourSequencerAddress"
export FLASHSTAT__TEE__ATTESTATION_ENABLED="false"
export FLASHSTAT__GUARDIAN__PRIVATE_KEY="0xYourPrivateKey"
export FLASHSTAT__GUARDIAN__SLASHING_CONTRACT="0xSlashingManagerAddress"
```

### Full reference

| Key | Type | Required | Default | Description |
|---|---|---|---|---|
| `rpc.ws_url` | String | ✅ | — | WebSocket endpoint for block subscription |
| `rpc.http_url` | String | ✅ | — | HTTP endpoint for polling fallback and on-chain calls |
| `storage.db_path` | String | ✅ | — | Filesystem path for the embedded redb database |
| `tee.sequencer_address` | Address | ✅ | — | Expected Ethereum address of the Unichain sequencer |
| `tee.attestation_enabled` | bool | ✅ | `false` | Enable Intel TDX attestation quote verification |
| `tee.expected_mrenclave` | String | ❌ | — | Hex-encoded expected MRENCLAVE for TDX quote pinning |
| `guardian.private_key` | String | ❌ | — | Raw hex private key for the Guardian Wallet |
| `guardian.keystore_path` | String | ❌ | — | Path to an ERC-55 keystore JSON file |
| `guardian.slashing_contract` | Address | ✅ | — | Address of the deployed `SlashingManager` contract |

> **Note:** If both `private_key` and `keystore_path` are absent, the Guardian Wallet is disabled. The monitor will still detect and log equivocations but will not submit on-chain proofs.

---

## Running the System

### 1. Core Monitor (`flashstat`)

Connects to Unichain over WebSocket, processes new blocks, and logs all detections to stdout.

```bash
cargo run --release --bin flashstat
```

The monitor uses a **supervisor loop**: if the WebSocket drops, it automatically falls back to HTTP polling at 2-second intervals and retries the WebSocket connection on each supervisor cycle.

---

### 2. RPC Server (`flashstat-server`)

Starts the `FlashMonitor` internally and exposes a JSON-RPC 2.0 server with WebSocket pub/sub at `127.0.0.1:9944`.

```bash
cargo run --release --bin flashstat-server
```

This is the process all other clients (TUI, simulator, external tools) connect to. The server also tracks live stats (uptime, total blocks, total reorgs, DB size) in memory.

---

### 3. Terminal Dashboard (`flashstat-tui`)

A full-featured terminal UI that connects to `flashstat-server` at `http://127.0.0.1:9944` and renders a live dashboard.

```bash
cargo run --release --bin flashstat-tui
```

**Keyboard controls:**

| Key | Action |
|---|---|
| `q` | Quit |
| `↑` / `↓` | Scroll the sequencer leaderboard |

**Dashboard panels:**

| Panel | Content |
|---|---|
| **Block Feed** | Live stream of ingested blocks — number, confidence %, hash, status badge |
| **Sequencer Reputation** | Ranked leaderboard — score, total blocks, streak, attestations, misbehaviour counts |
| **Reorg Log** | Recent reorg events — severity, block number, conflicting hashes |
| **System Health** | Uptime, total blocks processed, reorg count, DB size |

---

### 4. Forensic Simulator (`flashstat-simulate`)

Injects synthetic block scenarios into a running `flashstat-server` via the `flash_ingestBlock` RPC method, for testing and demonstration.

```bash
# Simulate 3 equivocation scenarios (default)
cargo run --release --bin flashstat-simulate -- --count 3

# Simulate 5 standard sequential blocks
cargo run --release --bin flashstat-simulate -- --count 5 --severity standard

# Target a non-default server
cargo run --release --bin flashstat-simulate -- --url http://192.168.1.10:9944
```

**Arguments:**

| Flag | Default | Description |
|---|---|---|
| `--url` / `-u` | `http://127.0.0.1:9944` | Target RPC server URL |
| `--count` / `-c` | `1` | Number of scenarios to inject |
| `--severity` / `-s` | `equivocation` | Scenario type: `equivocation` or `standard` |

An `equivocation` simulation injects two blocks at the same height with different hashes and different `extra_data` signatures, triggering reorg detection, reputation penalties, and (if configured) on-chain proof submission.

---

## Guardian Wallet (Active Protection)

When a Guardian Wallet is configured, the monitor **automatically submits on-chain slashing proofs** upon detecting an equivocation. This is the Active Watchtower mode.

The guardian wallet submits two proof types:

- **Equivocation proof**: RLP-encoded block number, signer address, two conflicting signatures, and two conflicting block hashes
- **Double-spend proof**: RLP-encoded conflicting transaction hash pair, sender address, and nonce

### Private-key mode

```toml
[guardian]
private_key       = "0xabc123..."
slashing_contract = "0xSlashingManager"
```

> ⚠️ **Security warning:** Do not commit private keys to version control. Prefer keystore mode for all production deployments.

### Keystore mode

```toml
[guardian]
keystore_path     = "/secure/path/keystore.json"
slashing_contract = "0xSlashingManager"
```

Set the keystore password via environment variable (never in the config file):

```bash
export FLASHSTAT__GUARDIAN__PASSWORD="your-keystore-password"
```

### Slashing contract ABI

The `SlashingManager` contract must implement:

```solidity
function submitEquivocationProof(bytes calldata proof) external;
function submitDoubleSpendProof(bytes calldata proof) external;
```

See [Proof Encoding](#proof-encoding) for the exact RLP encoding format of the `proof` argument.

---

## JSON-RPC API Reference

All methods use the `flash_` namespace. The server binds to `127.0.0.1:9944` by default.

- **HTTP**: `http://127.0.0.1:9944`
- **WebSocket**: `ws://127.0.0.1:9944`

### Methods

#### `flash_getLatestBlock`
Returns the most recently processed `FlashBlock`, or `null` if the monitor has not yet seen any blocks.

```json
{ "jsonrpc": "2.0", "id": 1, "method": "flash_getLatestBlock", "params": [] }
```

**Response:**
```json
{
  "number": "0x3B9AC9FF",
  "hash": "0xabc...",
  "parent_hash": "0xdef...",
  "confidence": 95.5,
  "status": "Stable",
  "signer": "0xSequencerAddress",
  "timestamp": "2024-01-01T00:00:00Z"
}
```

---

#### `flash_getRecentBlocks`
Returns the `N` most recently processed blocks, newest first.

```json
{ "jsonrpc": "2.0", "id": 1, "method": "flash_getRecentBlocks", "params": [10] }
```

---

#### `flash_getConfidence`
Returns the `confidence` score (`0.0–100.0`) for a specific block hash. Returns `0.0` for unknown blocks.

```json
{ "jsonrpc": "2.0", "id": 1, "method": "flash_getConfidence", "params": ["0xBlockHash"] }
```

---

#### `flash_getRecentReorgs`
Returns the `N` most recent reorg events (both soft reorgs and equivocations).

```json
{ "jsonrpc": "2.0", "id": 1, "method": "flash_getRecentReorgs", "params": [20] }
```

---

#### `flash_getEquivocations`
Returns the `N` most recent events classified as `Equivocation` severity only.

```json
{ "jsonrpc": "2.0", "id": 1, "method": "flash_getEquivocations", "params": [10] }
```

---

#### `flash_getSequencerRankings`
Returns all tracked sequencer addresses, ranked by reputation score descending.

```json
{ "jsonrpc": "2.0", "id": 1, "method": "flash_getSequencerRankings", "params": [] }
```

---

#### `flash_getHealth`
Returns the current system health snapshot.

```json
{ "jsonrpc": "2.0", "id": 1, "method": "flash_getHealth", "params": [] }
```

**Response:**
```json
{
  "uptime_secs": 3600,
  "total_blocks": 12500,
  "total_reorgs": 3,
  "db_size_bytes": 2097152
}
```

---

#### `flash_ingestBlock`
Manually injects a raw Ethereum block into the monitor pipeline. Used by the forensic simulator and integration tests.

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "flash_ingestBlock",
  "params": [{ "number": "0x...", "hash": "0x...", "parentHash": "0x...", "timestamp": "0x..." }]
}
```

---

### Subscriptions

WebSocket only. Use `eth_subscribe`-style jsonrpsee subscriptions.

#### `flash_subscribeBlocks`
Streams a `FlashBlock` notification for every block processed by the monitor.

#### `flash_subscribeEvents`
Streams a `ReorgEvent` notification every time a soft reorg or equivocation is detected.

---

## Confidence Model

Every block processed by the monitor is assigned a `confidence` score (`0.0–100.0`) representing the probability that the block is canonical and will not be reorganised away.

The model uses two independent signals:

### 1. Persistence depth

Confidence grows as blocks at higher heights are observed without conflict:

```
base_confidence = (1 - 0.5^persistence) × 100
```

`persistence` increments each time a new non-conflicting block at a higher height is observed. A block's prior confidence feeds the next block's persistence estimate.

### 2. TEE verification override

If the block carries a valid ECDSA signature from the expected sequencer in its `extra_data`, the TEE signal overrides the persistence estimate entirely:

| Condition | Confidence |
|---|---|
| No signature found | Persistence-based only |
| Signature from wrong signer | Persistence-based only |
| Valid sequencer signature, attestation disabled | **90%** |
| Attestation enabled, quote missing | **85%** |
| Attestation enabled, TDX check failed | **45%** |
| Attestation enabled, TDX check errored | **70%** |
| Attestation enabled, TDX Quote V4 valid | **99%** |

A block with `confidence > 95.0` is classified as **`Stable`**. All others remain **`Pending`**.

---

## Reputation Scoring

Each sequencer address accumulates a `reputation_score` computed from all observed events:

```
score = total_blocks_signed
      + total_attested_blocks          ← permanent +1 per hardware-attested block
      + (current_streak / 100) × 10   ← streak bonus: +10 for every 100-block streak
      − (total_soft_reorgs × 50)      ← soft reorg deduction
      − (total_equivocations × 1000)  ← equivocation deduction (20× heavier)
```

**Penalty rules:**
- Any misbehaviour (soft reorg or equivocation) **resets the signing streak to zero**
- A single equivocation against a sequencer with a long history will typically result in a deeply negative score
- Scores are recalculated in full on every event — no incremental drift

The leaderboard is exposed via `flash_getSequencerRankings` and rendered in the TUI dashboard.

---

## TEE / TDX Attestation

FlashStat includes a `TeeVerifier` that performs two layers of hardware-attestation verification when `tee.attestation_enabled = true`.

### Layer 1: ECDSA Signature Recovery

The sequencer signature is extracted from the **last 65 bytes** of the block's `extra_data` field (matching the OP-Stack Flashblocks convention). The ECDSA signer is recovered via `ecrecover` and compared against `tee.sequencer_address`.

### Layer 2: Intel TDX Quote V4 Verification

If a TDX attestation quote is present in `extra_data` after the signature (bytes `[97..]`), a structural verification is performed:

1. **Header check**: Quote version must be `4` (TDX Quote V4); attestation type must be `2` (TDX)
2. **MRENCLAVE pinning** (optional): If `tee.expected_mrenclave` is set, the 32-byte MRENCLAVE at quote offset `96–128` is compared against the configured hex value byte-for-byte

> **Production note:** This implementation performs structural and MRENCLAVE validation. For full quote verification against Intel's Attestation Service, integrate with Intel's DCAP library or a remote attestation service such as [Intel Trust Authority](https://www.intel.com/content/www/us/en/security/trust-authority.html).

---

## Proof Encoding

Both proof types submitted to the `SlashingManager` contract are RLP-encoded.

### Equivocation Proof

```
RLP([
    block_number,   ← U256
    signer,         ← Address (20 bytes)
    signature_1,    ← Bytes (65 bytes, ECDSA from old block)
    signature_2,    ← Bytes (65 bytes, ECDSA from new block)
    block_hash_1,   ← H256
    block_hash_2    ← H256
])
```

### Double-Spend Proof

```
RLP([
    tx_hash_1,  ← H256 (original transaction)
    tx_hash_2,  ← H256 (replacement transaction at same nonce)
    sender,     ← Address (20 bytes)
    nonce       ← U256
])
```

---

## Crate Reference

| Crate | Role |
|---|---|
| `flashstat-common` | Shared data types (`FlashBlock`, `ReorgEvent`, `SequencerStats`, `Config`) and config loader |
| `flashstat-db` | `FlashStorage` trait + `RedbStorage` implementation (embedded redb) |
| `flashstat-api` | `jsonrpsee` `#[rpc]` proc-macro trait — generates both server and client stubs |
| `flashstat-core` | `FlashMonitor`, `TeeVerifier`, `GuardianWallet`, RLP proof encoders |
| `flashstat` (bin) | Standalone core monitor entry point |
| `flashstat-server` (bin) | JSON-RPC server wrapping the monitor |
| `flashstat-tui` (bin) | `ratatui`-based terminal dashboard |
| `flashstat-simulate` (bin) | CLI forensic simulation tool |
| `flashstat-watchtower-test` (bin + tests) | Manual smoke-test binary + Cargo integration test harness |

---

## Testing

Run the full suite:

```bash
cargo test --all-features
```

The suite contains **27 tests** across three categories:

### Unit tests — `flashstat-db` (14 tests)

Tests all `RedbStorage` operations against a temporary database:

- Block save, get by hash, latest block tracking, recent blocks ordering and limit enforcement
- Reorg save and retrieval, equivocation-only filtering
- Sequencer stats upsert, overwrite, and full enumeration

### Unit tests — `flashstat-core` (13 tests)

Tests monitoring logic, reputation engine, and extraction helpers:

- Reputation scoring, streak bonus calculation, soft reorg vs. equivocation penalty comparison
- `handle_new_block`: block persistence, latest block tracking, broadcast channel emission
- Reorg detection: conflicting hashes trigger an event; sequential blocks do not (no false positives); reorg events are persisted
- `extract_signature_from_block`: correct 65-byte tail extraction; short `extra_data` returns `None`
- `extract_quote_from_block`: correct extraction from offset 97; absent quote returns `None`
- `encode_equivocation_proof` and `encode_double_spend_proof` serialization round-trips

### Integration tests — `flashstat-watchtower-test` (11 tests)

End-to-end pipeline tests using an in-process monitor and temporary database — **no live RPC node required**:

- Single block ingestion and retrieval
- Sequential blocks produce no reorg events
- Latest block always reflects the most recently ingested block
- Soft reorg detection on hash conflict at the same height; persistence to storage; duplicate-hash idempotency
- Reputation accumulation; equivocation penalty drives score negative; streak bonus calculation
- Broadcast channel fires for every ingested block
- Full watchtower equivocation scenario: two conflicting signed blocks → reorg event emitted and persisted

---

## CI

GitHub Actions runs on every push to `main` and `dean`, and on all pull requests targeting those branches.

`.github/workflows/ci.yml` runs two jobs:

| Job | Steps |
|---|---|
| **Check & Lint** | `cargo fmt --all -- --check` → `cargo clippy --all-targets --all-features -- -D warnings` |
| **Test** | `cargo test --all-features` |

All Clippy warnings are treated as hard errors in CI.

---

## Development Notes

### Logging

All binaries use `tracing-subscriber`. Control the log level via `RUST_LOG`:

```bash
RUST_LOG=flashstat_core=debug,flashstat_server=info cargo run --bin flashstat-server
```

### Adding a new RPC method

1. Add the method signature to `crates/flashstat-api/src/lib.rs` inside the `#[rpc]` trait
2. Implement it on `FlashServer` in `bin/flashstat-server/src/main.rs`
3. The `flashstat-api` proc macro auto-generates both the server trait and the `FlashApiClient` stub

### Adding a new storage operation

1. Add the method to the `FlashStorage` trait in `crates/flashstat-db/src/lib.rs`
2. Implement it on `RedbStorage` in the same file
3. Add a corresponding `#[tokio::test]` in the `mod tests` block of the same file

### Configuration precedence

`flashstat.toml` < environment variables. There is no CLI flag layer — all runtime configuration flows exclusively through the config file and environment.

### Database

FlashStat uses [redb](https://github.com/cberner/redb), a pure-Rust embedded key-value store. The database file is a single file at `storage.db_path`. It is safe to copy for backup. Delete the file to reset all state.
