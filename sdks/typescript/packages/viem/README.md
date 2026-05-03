# @flashstat/viem

A lightweight [Viem](https://viem.sh) extension for **FlashStat**, enabling cryptographic block confidence checks directly on your Viem Public Client.

## Installation

```bash
npm install @flashstat/viem @flashstat/core
```

## Features

- **Seamless Integration**: Extends any standard Viem client with `flashStatActions`.
- **Familiar API**: Uses the same patterns as other Viem extensions.
- **Type-Safe**: Full type inference for all extended actions.

## Quick Start

```typescript
import { createPublicClient, http } from 'viem';
import { mainnet } from 'viem/chains';
import { flashStatActions } from '@flashstat/viem';

const client = createPublicClient({
  chain: mainnet,
  transport: http(),
}).extend(flashStatActions({ url: 'http://localhost:9944' }));

// Use standard Viem client with FlashStat powers!
const confidence = await client.getFlashConfidence('0xabc...');
const latestBlock = await client.getLatestFlashBlock();
const rankings = await client.getFlashSequencerRankings();
```

## Available Actions

- `getFlashConfidence(hash)`
- `getLatestFlashBlock()`
- `getFlashRecentReorgs(limit)`
- `getFlashEquivocations(limit)`
- `getFlashHealth()`
- `getFlashSequencerRankings()`

## License

MIT
