/**
 * @flashstat/viem
 *
 * Extends any Viem client with FlashStat actions for querying real-time
 * Flashblock confidence and sequencer reputation on Unichain.
 *
 * @example
 * ```typescript
 * import { createPublicClient, http } from 'viem';
 * import { unichain } from 'viem/chains';
 * import { flashStatActions } from '@flashstat/viem';
 *
 * const client = createPublicClient({ chain: unichain, transport: http() })
 *   .extend(flashStatActions({ url: 'http://127.0.0.1:9944' }));
 *
 * const confidence = await client.getFlashConfidence('0xabc...');
 * ```
 */
export { flashStatActions } from "./actions.js";
export type { FlashStatActions, FlashStatActionsConfig } from "./actions.js";

// Re-export core types for convenience so consumers only need one import.
export type {
  FlashBlock,
  ReorgEvent,
  SequencerStats,
  SystemHealth,
  BlockStatus,
  ReorgSeverity,
} from "@flashstat/core";
