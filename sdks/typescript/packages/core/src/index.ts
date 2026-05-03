/**
 * @flashstat/core
 *
 * Zero-dependency TypeScript client for the FlashStat JSON-RPC & WebSocket API.
 *
 * @example
 * ```typescript
 * import { FlashStatClient } from '@flashstat/core';
 *
 * const client = new FlashStatClient({ url: 'http://127.0.0.1:9944' });
 * const confidence = await client.getConfidence('0xabc...');
 * ```
 */
export { FlashStatClient, FlashStatError } from "./client.js";
export type { FlashStatClientConfig } from "./client.js";

export type {
  BlockStatus,
  ConflictAnalysis,
  DoubleSpendProof,
  EquivocationEvent,
  FlashBlock,
  ReorgEvent,
  ReorgSeverity,
  SequencerStats,
  SystemHealth,
  UnsubscribeFn,
} from "./types.js";
