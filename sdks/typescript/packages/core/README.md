# @flashstat/core

> **Zero-dependency TypeScript client for FlashStat — the cryptographic block confidence monitor for Unichain.**

FlashStat provides real-time, TEE-attested block confidence and sequencer reputation data. This core library allows any JavaScript or TypeScript application to interface directly with the FlashStat JSON-RPC and WebSocket API.

## 🚀 Why FlashStat?

In the Unichain ecosystem, **Flashblocks** provide fast pre-confirmations. FlashStat adds a layer of verifiable security by monitoring sequencer behavior from inside a Trusted Execution Environment (TEE).

- **Real-time Confidence**: Know exactly how "safe" a block is before it's finalized on L1.
- **Sequencer Accountability**: Track sequencer reputation and detect double-signing (equivocation) instantly.
- **Low Latency**: Built on a high-performance Rust backend with WebSocket streaming.

## 📦 Installation

```bash
npm install @flashstat/core
```

## 🛠 Features

- **Zero Dependencies**: Pure TypeScript implementation using native `fetch` and `WebSocket`.
- **Full Type Safety**: Complete interfaces for `FlashBlock`, `ReorgEvent`, and `SequencerStats`.
- **Reliable Subscriptions**: Automatic reconnection logic for stable real-time monitoring.
- **Universal**: Works in Node.js, Browsers, and Edge environments.

## 🚦 Quick Start

### 1. Initialize the Client
```typescript
import { FlashStatClient } from '@flashstat/core';

const client = new FlashStatClient({
  url: 'http://localhost:9944',   // FlashStat RPC URL
  timeoutMs: 5000,                // Optional timeout
});
```

### 2. Request Block Confidence
```typescript
const hash = '0x...';
const confidence = await client.getConfidence(hash);

console.log(`Block confidence: ${confidence}%`);
```

### 3. Subscribe to Live Blocks
```typescript
const unsub = client.subscribeBlocks((block) => {
  console.log(`New block ${block.number} arrived with ${block.confidence}% confidence.`);
  
  if (block.confidence > 95) {
    console.log("Safe to show to user!");
  }
});

// To stop listening:
// unsub();
```

## 📖 API Reference

### Data Methods (HTTP)
| Method | Description |
|--------|-------------|
| `getConfidence(hash)` | Returns a score (0.0-100.0) for a specific block hash. |
| `getLatestBlock()` | Returns the most recent `FlashBlock` seen by the monitor. |
| `getRecentBlocks(limit)` | Returns a history of the last N blocks. |
| `getRecentReorgs(limit)` | Returns recent reorg or equivocation events. |
| `getSequencerRankings()` | Returns the global sequencer reputation leaderboard. |
| `getHealth()` | Returns the node's system health and uptime. |

### Subscription Methods (WebSocket)
| Method | Description |
|--------|-------------|
| `subscribeBlocks(cb)` | Streams every new block processed by the monitor. |
| `subscribeEvents(cb)` | Streams reorg alerts and sequencer misbehavior. |

## 🔗 Links

- **Main Repository**: [github.com/One-Block-Org/FlashStat](https://github.com/One-Block-Org/FlashStat)
- **Documentation**: [FlashStat Wiki](https://github.com/One-Block-Org/FlashStat#readme)
- **Example App**: [Next.js Dashboard Example](https://github.com/One-Block-Org/FlashStat/tree/main/sdks/typescript/examples/nextjs-dashboard)

## 📄 License

MIT © [One Block](https://github.com/One-Block-Org)
