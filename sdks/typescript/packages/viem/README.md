# @flashstat/viem

> **Native Viem Actions for FlashStat — Cryptographic block confidence directly in your Ethereum client.**

This package extends [Viem](https://viem.sh) with custom actions to interact with a **FlashStat** monitor. It allows you to gate your dApp's UI or backend logic on TEE-attested block confidence scores.

## 📦 Installation

```bash
npm install @flashstat/viem @flashstat/core
```

## 🚀 Usage

Extend your existing Viem `PublicClient` with `flashStatActions`.

```typescript
import { createPublicClient, http } from 'viem';
import { mainnet } from 'viem/chains';
import { flashStatActions } from '@flashstat/viem';

const client = createPublicClient({
  chain: mainnet,
  transport: http(),
}).extend(flashStatActions({ 
  url: 'http://localhost:9944' 
}));

// Now use FlashStat methods alongside standard Viem methods
async function checkTransaction(hash: `0x${string}`) {
  const confidence = await client.getFlashConfidence(hash);
  
  if (confidence > 99.9) {
    console.log("Transaction is cryptographically secure via FlashStat!");
  }
}
```

## 🛠 Available Actions

All actions are prefixed with `getFlash` or similar to avoid collisions with standard Viem methods:

- **`getFlashConfidence(hash)`**: Fetch the 0-100 confidence score for any hash.
- **`getLatestFlashBlock()`**: Get the most recent TEE-monitored block.
- **`getFlashRecentReorgs(limit)`**: Get the history of soft or deep reorgs.
- **`getFlashEquivocations(limit)`**: Filter specifically for sequencer double-signing events.
- **`getFlashSequencerRankings()`**: View the reputation leaderboard for Unichain sequencers.
- **`getFlashHealth()`**: Check the status of the FlashStat infrastructure node.

## 📖 Why use the Viem Extension?

By using `@flashstat/viem`, you keep your codebase idiomatic. You don't need to manage a separate `FlashStatClient` instance; all the data you need for Unichain block confidence is available directly on your primary blockchain client.

## 🔗 Links

- **Main Repository**: [github.com/One-Block-Org/FlashStat](https://github.com/One-Block-Org/FlashStat)
- **Core Library**: [@flashstat/core](https://www.npmjs.com/package/@flashstat/core)
- **React Hooks**: [@flashstat/react](https://www.npmjs.com/package/@flashstat/react)

## 📄 License

MIT © [One Block](https://github.com/One-Block-Org)
