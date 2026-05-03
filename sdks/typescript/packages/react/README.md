# @flashstat/react

> **React Hooks and Context for FlashStat — Real-time Unichain block confidence for modern dApps.**

`@flashstat/react` provides a seamless way to integrate **FlashStat's** real-time TEE-attested data into your React applications. It manages WebSocket connections, subscription state, and loading cycles automatically.

## 📦 Installation

```bash
npm install @flashstat/react @flashstat/core
```

## 🚀 Quick Start

### 1. Setup the Provider
Wrap your application (usually in `layout.tsx` or `_app.tsx`) with the `FlashStatProvider`.

```tsx
import { FlashStatProvider } from '@flashstat/react';

export default function RootLayout({ children }) {
  return (
    <html lang="en">
      <body>
        <FlashStatProvider url="http://localhost:9944">
          {children}
        </FlashStatProvider>
      </body>
    </html>
  );
}
```

### 2. Use the Hooks
Access live-streaming data anywhere in your component tree.

```tsx
import { useFlashConfidence, useLatestFlashBlock } from '@flashstat/react';

export function ConfidenceBadge({ hash }: { hash: string }) {
  const { confidence, status, isLoading } = useFlashConfidence(hash);

  if (isLoading) return <span>Loading...</span>;

  return (
    <div className={`badge ${status}`}>
      Confidence: {confidence.toFixed(2)}%
    </div>
  );
}

export function LiveFeed() {
  const { block } = useLatestFlashBlock();

  return (
    <div>
      <h3>Latest Unichain Block: {block?.number}</h3>
      <p>Hash: {block?.hash}</p>
    </div>
  );
}
```

## 🛠 Available Hooks

- **`useLatestFlashBlock()`**: Streams the most recent block processed by the monitor.
- **`useFlashConfidence(hash)`**: Returns a live-updating confidence score for a specific block hash.
- **`useReorgEvents(limit)`**: Provides a stream of reorg and sequencer equivocation alerts.
- **`useSequencerRankings(pollInterval)`**: Returns the live reputation leaderboard.
- **`useFlashHealth()`**: Monitors the health and uptime of the connected FlashStat node.

## 💡 Why use the React Hooks?

- **Efficient Connectivity**: Opens only one WebSocket connection shared across all hooks.
- **Automatic Lifecycle**: Handles connection opening on mount and cleanup on unmount.
- **Reactive UI**: Your UI updates instantly as the TEE publishes new confidence attestations.

## 🔗 Links

- **Main Repository**: [github.com/One-Block-Org/FlashStat](https://github.com/One-Block-Org/FlashStat)
- **Demo Dashboard**: [Next.js Reference Implementation](https://github.com/One-Block-Org/FlashStat/tree/main/sdks/typescript/examples/nextjs-dashboard)
- **Core SDK**: [@flashstat/core](https://www.npmjs.com/package/@flashstat/core)

## 📄 License

MIT © [One Block](https://github.com/One-Block-Org)
