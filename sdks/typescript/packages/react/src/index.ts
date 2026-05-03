/**
 * @flashstat/react
 *
 * React hooks and context provider for real-time Unichain Flashblock confidence
 * powered by FlashStat.
 *
 * @example
 * ```tsx
 * import { FlashStatProvider, useFlashConfidence } from '@flashstat/react';
 *
 * function App() {
 *   return (
 *     <FlashStatProvider url="http://127.0.0.1:9944">
 *       <TxStatus hash="0xabc..." />
 *     </FlashStatProvider>
 *   );
 * }
 *
 * function TxStatus({ hash }: { hash: string }) {
 *   const { confidence, status } = useFlashConfidence(hash);
 *   return <p>{status} — {confidence.toFixed(1)}%</p>;
 * }
 * ```
 */
export {
  FlashStatProvider,
  useFlashStatClient,
} from "./context.js";
export type { FlashStatProviderProps } from "./context.js";

export {
  useFlashConfidence,
  useLatestFlashBlock,
  useReorgEvents,
  useSequencerRankings,
  useFlashHealth,
} from "./hooks.js";
export type {
  UseFlashConfidenceResult,
  UseFlashHealthResult,
  UseLatestFlashBlockResult,
  UseReorgEventsResult,
  UseSequencerRankingsResult,
} from "./hooks.js";

// Re-export core types for convenience.
export type {
  BlockStatus,
  FlashBlock,
  ReorgEvent,
  ReorgSeverity,
  SequencerStats,
  SystemHealth,
} from "@flashstat/core";
