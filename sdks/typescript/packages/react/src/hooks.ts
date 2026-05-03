import { useCallback, useEffect, useRef, useState } from "react";
import type {
  BlockStatus,
  FlashBlock,
  ReorgEvent,
  SequencerStats,
  SystemHealth,
  UnsubscribeFn,
} from "@flashstat/core";
import { useFlashStatClient } from "./context.js";

// ─── Shared hook state shape ──────────────────────────────────────────────────

interface HookState<T> {
  data: T;
  isLoading: boolean;
  error: Error | null;
}

// ─── useFlashConfidence ───────────────────────────────────────────────────────

export interface UseFlashConfidenceResult {
  /** Confidence score from `0.0` (unknown) to `100.0` (fully attested). */
  confidence: number;
  /** Lifecycle status of the block. */
  status: BlockStatus | null;
  isLoading: boolean;
  error: Error | null;
}

/**
 * Returns real-time confidence for a specific block hash.
 *
 * The value is fetched immediately and then updated via a live WebSocket
 * subscription whenever new blocks arrive that affect this hash.
 *
 * @example
 * ```tsx
 * function TxStatus({ hash }: { hash: string }) {
 *   const { confidence, status, isLoading } = useFlashConfidence(hash);
 *   if (isLoading) return <Spinner />;
 *   return <p>{status} — {confidence.toFixed(1)}% confident</p>;
 * }
 * ```
 */
export function useFlashConfidence(hash: string): UseFlashConfidenceResult {
  const client = useFlashStatClient();
  const [confidence, setConfidence] = useState(0);
  const [status, setStatus] = useState<BlockStatus | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    if (!hash) return;

    let cancelled = false;
    setIsLoading(true);
    setError(null);

    // Initial fetch
    client
      .getConfidence(hash)
      .then((score) => {
        if (!cancelled) {
          setConfidence(score);
          setIsLoading(false);
        }
      })
      .catch((err: Error) => {
        if (!cancelled) {
          setError(err);
          setIsLoading(false);
        }
      });

    // Subscribe to live block updates — update when this specific block matures.
    const unsub: UnsubscribeFn = client.subscribeBlocks((block) => {
      if (block.hash === hash) {
        setConfidence(block.confidence);
        setStatus(block.status);
      }
    });

    return () => {
      cancelled = true;
      unsub();
    };
  }, [client, hash]);

  return { confidence, status, isLoading, error };
}

// ─── useLatestFlashBlock ──────────────────────────────────────────────────────

export interface UseLatestFlashBlockResult {
  block: FlashBlock | null;
  isLoading: boolean;
  error: Error | null;
}

/**
 * Returns the most recently processed {@link FlashBlock}, updating in real-time
 * via WebSocket subscription.
 *
 * @example
 * ```tsx
 * function LiveBlockFeed() {
 *   const { block } = useLatestFlashBlock();
 *   if (!block) return null;
 *   return <p>Block #{block.number} — {block.confidence.toFixed(1)}% confidence</p>;
 * }
 * ```
 */
export function useLatestFlashBlock(): UseLatestFlashBlockResult {
  const client = useFlashStatClient();
  const [block, setBlock] = useState<FlashBlock | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);

    client
      .getLatestBlock()
      .then((b) => {
        if (!cancelled) {
          setBlock(b);
          setIsLoading(false);
        }
      })
      .catch((err: Error) => {
        if (!cancelled) {
          setError(err);
          setIsLoading(false);
        }
      });

    const unsub = client.subscribeBlocks((b) => {
      if (!cancelled) setBlock(b);
    });

    return () => {
      cancelled = true;
      unsub();
    };
  }, [client]);

  return { block, isLoading, error };
}

// ─── useReorgEvents ───────────────────────────────────────────────────────────

export interface UseReorgEventsResult {
  events: ReorgEvent[];
  isLoading: boolean;
  error: Error | null;
}

/**
 * Returns recent reorg and equivocation events, updating in real-time
 * via WebSocket subscription.
 *
 * @param limit - Maximum number of historical events to fetch on mount. Defaults to `20`.
 *
 * @example
 * ```tsx
 * function ReorgAlert() {
 *   const { events } = useReorgEvents(5);
 *   const latest = events[0];
 *   if (!latest || latest.severity !== 'Equivocation') return null;
 *   return <Alert>Equivocation detected at block {latest.blockNumber}!</Alert>;
 * }
 * ```
 */
export function useReorgEvents(limit = 20): UseReorgEventsResult {
  const client = useFlashStatClient();
  const [events, setEvents] = useState<ReorgEvent[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);

    client
      .getRecentReorgs(limit)
      .then((initial) => {
        if (!cancelled) {
          setEvents(initial);
          setIsLoading(false);
        }
      })
      .catch((err: Error) => {
        if (!cancelled) {
          setError(err);
          setIsLoading(false);
        }
      });

    const unsub = client.subscribeEvents((event) => {
      if (!cancelled) {
        setEvents((prev) => [event, ...prev].slice(0, limit));
      }
    });

    return () => {
      cancelled = true;
      unsub();
    };
  }, [client, limit]);

  return { events, isLoading, error };
}

// ─── useSequencerRankings ─────────────────────────────────────────────────────

export interface UseSequencerRankingsResult {
  rankings: SequencerStats[];
  isLoading: boolean;
  error: Error | null;
  /** Manually trigger a refresh outside of the poll interval. */
  refresh: () => void;
}

/**
 * Returns the sequencer reputation leaderboard, polling at a configurable
 * interval (default: `10_000` ms).
 *
 * @param pollIntervalMs - How often to poll for updates. Defaults to `10000`.
 *
 * @example
 * ```tsx
 * function Leaderboard() {
 *   const { rankings } = useSequencerRankings(5_000);
 *   return (
 *     <ul>
 *       {rankings.map((s) => (
 *         <li key={s.address}>{s.address}: {s.reputationScore}</li>
 *       ))}
 *     </ul>
 *   );
 * }
 * ```
 */
export function useSequencerRankings(
  pollIntervalMs = 10_000,
): UseSequencerRankingsResult {
  const client = useFlashStatClient();
  const [rankings, setRankings] = useState<SequencerStats[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [tick, setTick] = useState(0);

  const refresh = useCallback(() => setTick((t) => t + 1), []);

  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);

    client
      .getSequencerRankings()
      .then((data) => {
        if (!cancelled) {
          setRankings(data);
          setIsLoading(false);
        }
      })
      .catch((err: Error) => {
        if (!cancelled) {
          setError(err);
          setIsLoading(false);
        }
      });

    const timer = setInterval(() => {
      client.getSequencerRankings().then((data) => {
        if (!cancelled) setRankings(data);
      }).catch(() => {/* silently ignore poll errors */});
    }, pollIntervalMs);

    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [client, pollIntervalMs, tick]);

  return { rankings, isLoading, error, refresh };
}

// ─── useFlashHealth ───────────────────────────────────────────────────────────

export interface UseFlashHealthResult {
  health: SystemHealth | null;
  isLoading: boolean;
  error: Error | null;
  refresh: () => void;
}

/**
 * Returns the current system health of the FlashStat node, polling at a
 * configurable interval (default: `30_000` ms).
 */
export function useFlashHealth(pollIntervalMs = 30_000): UseFlashHealthResult {
  const client = useFlashStatClient();
  const [health, setHealth] = useState<SystemHealth | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [tick, setTick] = useState(0);

  const refresh = useCallback(() => setTick((t) => t + 1), []);

  useEffect(() => {
    let cancelled = false;
    setIsLoading(true);

    client
      .getHealth()
      .then((data) => {
        if (!cancelled) {
          setHealth(data);
          setIsLoading(false);
        }
      })
      .catch((err: Error) => {
        if (!cancelled) {
          setError(err);
          setIsLoading(false);
        }
      });

    const timer = setInterval(() => {
      client.getHealth().then((data) => {
        if (!cancelled) setHealth(data);
      }).catch(() => {});
    }, pollIntervalMs);

    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [client, pollIntervalMs, tick]);

  return { health, isLoading, error, refresh };
}
