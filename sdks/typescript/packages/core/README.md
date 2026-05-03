# @flashstat/core

The zero-dependency TypeScript client for **FlashStat**, the cryptographic block confidence monitor for Unichain.

## Installation

```bash
npm install @flashstat/core
```

## Features

- **Zero Dependencies**: Lightweight and fast.
- **Type-Safe**: Complete TypeScript definitions for all FlashStat RPC methods.
- **WebSocket Support**: Real-time subscriptions for new blocks and reorg events.
- **Auto-Reconnect**: Robust internal management of WebSocket connections.

## Quick Start

```typescript
import { FlashStatClient } from '@flashstat/core';

const client = new FlashStatClient({ 
  url: 'http://localhost:9944' 
});

// Get confidence score for a block hash
const confidence = await client.getConfidence('0xabc...');
console.log(`Confidence: ${confidence}%`);

// Subscribe to live blocks
const unsub = client.subscribeBlocks((block) => {
  console.log('New FlashBlock:', block.number, block.confidence);
});

// Cleanup
// unsub();
// client.destroy();
```

## API Overview

### Request/Response
- `getConfidence(hash)`: Get score (0-100) for a specific block.
- `getLatestBlock()`: Get the most recent block seen by the monitor.
- `getRecentBlocks(limit)`: History of recent blocks.
- `getRecentReorgs(limit)`: List of detected reorgs/equivocations.
- `getSequencerRankings()`: Leaderboard of sequencer reputation scores.
- `getHealth()`: Node uptime and status.

### Subscriptions
- `subscribeBlocks(callback)`: Listen for every new block.
- `subscribeEvents(callback)`: Listen for reorgs and sequencer misbehavior.

## License

MIT
