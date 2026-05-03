import { FlashStatClient } from "@flashstat/core";
import type {
  FlashBlock,
  ReorgEvent,
  SequencerStats,
  SystemHealth,
} from "@flashstat/core";
import type { Client, Transport, Chain, Account } from "viem";

// ─── Config ───────────────────────────────────────────────────────────────────

export interface FlashStatActionsConfig {
  /**
   * Base URL of the FlashStat RPC server.
   * @example "http://127.0.0.1:9944"
   */
  url: string;
  /**
   * WebSocket URL for subscriptions.
   * Defaults to `url` with `http` replaced by `ws`.
   */
  wsUrl?: string;
  /** Request timeout in milliseconds. Defaults to `5000`. */
  timeoutMs?: number;
}

// ─── Action return types ─────────────────────────────────────────────────────

export interface FlashStatActions {
  /**
   * Returns the confidence score (`0.0`–`100.0`) for the given block hash.
   *
   * @example
   * ```typescript
   * const score = await client.getFlashConfidence('0xabc...');
   * if (score > 95) console.log('Safe to proceed');
   * ```
   */
  getFlashConfidence(hash: string): Promise<number>;

  /**
   * Returns the most recently processed Flashblock, or `null` if none seen yet.
   */
  getLatestFlashBlock(): Promise<FlashBlock | null>;

  /**
   * Returns the `limit` most recently processed Flashblocks, newest first.
   */
  getFlashRecentBlocks(limit: number): Promise<FlashBlock[]>;

  /**
   * Returns the `limit` most recent reorg or equivocation events.
   */
  getFlashRecentReorgs(limit: number): Promise<ReorgEvent[]>;

  /**
   * Returns only `Equivocation`-severity events (sequencer double-signing).
   */
  getFlashEquivocations(limit: number): Promise<ReorgEvent[]>;

  /**
   * Returns the current system health snapshot of the FlashStat node.
   */
  getFlashHealth(): Promise<SystemHealth>;

  /**
   * Returns all tracked sequencer addresses ranked by reputation score.
   */
  getFlashSequencerRankings(): Promise<SequencerStats[]>;
}

// ─── Extension factory ────────────────────────────────────────────────────────

/**
 * Extends any Viem `PublicClient` with FlashStat actions.
 *
 * @example
 * ```typescript
 * import { createPublicClient, http } from 'viem';
 * import { flashStatActions } from '@flashstat/viem';
 *
 * const client = createPublicClient({
 *   chain: unichain,
 *   transport: http(),
 * }).extend(flashStatActions({ url: 'http://127.0.0.1:9944' }));
 *
 * const confidence = await client.getFlashConfidence('0xabc...');
 * const latest     = await client.getLatestFlashBlock();
 * ```
 */
export function flashStatActions(
  config: FlashStatActionsConfig,
): <TClient extends Client<Transport, Chain | undefined, Account | undefined>>(
  client: TClient,
) => FlashStatActions {
  return (_client) => {
    // One shared FlashStatClient instance per extension.
    const flash = new FlashStatClient({
      url: config.url,
      ...(config.wsUrl !== undefined && { wsUrl: config.wsUrl }),
      ...(config.timeoutMs !== undefined && { timeoutMs: config.timeoutMs }),
    });

    return {
      getFlashConfidence: (hash) => flash.getConfidence(hash),
      getLatestFlashBlock: () => flash.getLatestBlock(),
      getFlashRecentBlocks: (limit) => flash.getRecentBlocks(limit),
      getFlashRecentReorgs: (limit) => flash.getRecentReorgs(limit),
      getFlashEquivocations: (limit) => flash.getEquivocations(limit),
      getFlashHealth: () => flash.getHealth(),
      getFlashSequencerRankings: () => flash.getSequencerRankings(),
    };
  };
}
