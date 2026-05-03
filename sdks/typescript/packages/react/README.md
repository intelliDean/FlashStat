# @flashstat/react

React hooks and context provider for **FlashStat**. Build real-time, high-confidence dApps on Unichain with ease.

## Installation

```bash
npm install @flashstat/react @flashstat/core
```

## Features

- **Real-time Hooks**: Live-updating confidence scores and block data via WebSockets.
- **Shared State**: The `FlashStatProvider` manages a single optimized connection for your entire app.
- **Simple API**: Easy-to-use hooks like `useFlashConfidence` and `useLatestFlashBlock`.

## Quick Start

### 1. Wrap your App

```tsx
import { FlashStatProvider } from '@flashstat/react';

function App() {
  return (
    <FlashStatProvider url="http://localhost:9944">
      <YourDashboard />
    </FlashStatProvider>
  );
}
```

### 2. Use the Hooks

```tsx
import { useFlashConfidence, useLatestFlashBlock } from '@flashstat/react';

function YourDashboard() {
  const { block } = useLatestFlashBlock();
  const { confidence, status } = useFlashConfidence(block?.hash);

  return (
    <div>
      <p>Latest Block: {block?.number}</p>
      <p>Confidence: {confidence}% ({status})</p>
    </div>
  );
}
```

## Available Hooks

- `useFlashConfidence(hash)`: Returns live-updating confidence for a hash.
- `useLatestFlashBlock()`: Returns the most recent block from the stream.
- `useReorgEvents(limit)`: List of recent reorg alerts.
- `useSequencerRankings(pollInterval)`: Live leaderboard of sequencer scores.
- `useFlashHealth()`: Node health monitoring.

## License

MIT
