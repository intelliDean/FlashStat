import type { FlashBlock, ReorgEvent, UnsubscribeFn } from "./types.js";

// ─── JSON-RPC helpers ─────────────────────────────────────────────────────────

let _idCounter = 1;

function nextId(): number {
  return _idCounter++;
}

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number;
  method: string;
  params: unknown[];
}

interface JsonRpcSuccess<T> {
  jsonrpc: "2.0";
  id: number;
  result: T;
}

interface JsonRpcError {
  jsonrpc: "2.0";
  id: number;
  error: { code: number; message: string; data?: unknown };
}

type JsonRpcResponse<T> = JsonRpcSuccess<T> | JsonRpcError;

function isError<T>(r: JsonRpcResponse<T>): r is JsonRpcError {
  return "error" in r;
}

// ─── Subscription notification shape ─────────────────────────────────────────

interface JsonRpcNotification<T> {
  jsonrpc: "2.0";
  method: string;
  params: { subscription: string; result: T };
}

// ─── Public error class ───────────────────────────────────────────────────────

/**
 * Thrown by {@link FlashStatClient} when the server returns a JSON-RPC error
 * or when the transport layer fails.
 */
export class FlashStatError extends Error {
  constructor(
    message: string,
    /** JSON-RPC error code, or -1 for transport-level errors. */
    public readonly code: number,
    public readonly data?: unknown,
  ) {
    super(message);
    this.name = "FlashStatError";
  }
}

// ─── Subscription manager (internal) ─────────────────────────────────────────

type AnyCallback = (value: unknown) => void;

interface PendingSubscription {
  method: string;
  callback: AnyCallback;
}

/**
 * Manages a single WebSocket connection with automatic reconnection.
 * Callers register callbacks keyed by the subscription method name.
 */
class SubscriptionManager {
  private ws: WebSocket | null = null;
  private subscriptions = new Map<string, Set<AnyCallback>>();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private destroyed = false;

  constructor(
    private readonly wsUrl: string,
    private readonly reconnectMs: number,
  ) {}

  /**
   * Subscribe to a named method's notifications.
   * Returns an unsubscribe function.
   */
  subscribe<T>(method: string, callback: (value: T) => void): UnsubscribeFn {
    if (!this.subscriptions.has(method)) {
      this.subscriptions.set(method, new Set());
    }
    const cb = callback as AnyCallback;
    this.subscriptions.get(method)!.add(cb);

    // Lazily open the socket on first subscription.
    if (!this.ws || this.ws.readyState === WebSocket.CLOSED) {
      this.connect();
    }

    return () => {
      this.subscriptions.get(method)?.delete(cb);
    };
  }

  private connect(): void {
    if (this.destroyed) return;

    this.ws = new WebSocket(this.wsUrl);

    this.ws.addEventListener("open", () => {
      // Re-register all active subscriptions after a reconnect.
      for (const method of this.subscriptions.keys()) {
        this.sendSubscription(method);
      }
    });

    this.ws.addEventListener("message", (event: MessageEvent) => {
      let data: JsonRpcNotification<unknown>;
      try {
        data = JSON.parse(event.data as string) as JsonRpcNotification<unknown>;
      } catch {
        return; // Ignore malformed frames.
      }

      if (data.method && data.params?.result !== undefined) {
        const cbs = this.subscriptions.get(data.method);
        if (cbs) {
          for (const cb of cbs) {
            cb(data.params.result);
          }
        }
      }
    });

    this.ws.addEventListener("close", () => {
      if (!this.destroyed && this.subscriptions.size > 0) {
        this.reconnectTimer = setTimeout(() => this.connect(), this.reconnectMs);
      }
    });
  }

  private sendSubscription(method: string): void {
    if (this.ws?.readyState !== WebSocket.OPEN) return;
    const request: JsonRpcRequest = {
      jsonrpc: "2.0",
      id: nextId(),
      method,
      params: [],
    };
    this.ws.send(JSON.stringify(request));
  }

  destroy(): void {
    this.destroyed = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
    }
    this.ws?.close();
    this.subscriptions.clear();
  }
}

// ─── Client config ────────────────────────────────────────────────────────────

export interface FlashStatClientConfig {
  /**
   * Base URL of the FlashStat RPC server (e.g. `"http://127.0.0.1:9944"`).
   * Used for all HTTP request/response methods.
   */
  url: string;
  /**
   * WebSocket URL for subscriptions (e.g. `"ws://127.0.0.1:9944"`).
   * Defaults to `url` with the `http(s)` scheme replaced by `ws(s)`.
   */
  wsUrl?: string;
  /** Request timeout in milliseconds. Defaults to `5000`. */
  timeoutMs?: number;
  /** Auto-reconnect delay in milliseconds for subscriptions. Defaults to `2000`. */
  reconnectMs?: number;
}

// ─── FlashStatClient ─────────────────────────────────────────────────────────

/**
 * Primary entry point for interacting with a FlashStat node.
 *
 * @example
 * ```typescript
 * const client = new FlashStatClient({ url: 'http://127.0.0.1:9944' });
 *
 * const confidence = await client.getConfidence('0xabc...');
 * console.log(`Block confidence: ${confidence}%`);
 *
 * const unsub = client.subscribeBlocks((block) => {
 *   console.log('New block:', block.number, block.confidence);
 * });
 *
 * // Later…
 * unsub();
 * client.destroy();
 * ```
 */
export class FlashStatClient {
  private readonly httpUrl: string;
  private readonly wsUrl: string;
  private readonly timeoutMs: number;
  private readonly subscriptionManager: SubscriptionManager;

  constructor(config: FlashStatClientConfig) {
    this.httpUrl = config.url.replace(/\/$/, "");
    this.wsUrl =
      config.wsUrl ??
      this.httpUrl.replace(/^http/, "ws").replace(/^https/, "wss");
    this.timeoutMs = config.timeoutMs ?? 5_000;
    this.subscriptionManager = new SubscriptionManager(
      this.wsUrl,
      config.reconnectMs ?? 2_000,
    );
  }

  // ── Private ────────────────────────────────────────────────────────────────

  private async rpc<T>(method: string, params: unknown[] = []): Promise<T> {
    const body: JsonRpcRequest = {
      jsonrpc: "2.0",
      id: nextId(),
      method,
      params,
    };

    const controller = new AbortController();
    const timer = setTimeout(
      () => controller.abort(),
      this.timeoutMs,
    );

    let response: Response;
    try {
      response = await fetch(this.httpUrl, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: controller.signal,
      });
    } catch (err) {
      throw new FlashStatError(
        `Request to ${this.httpUrl} failed: ${(err as Error).message}`,
        -1,
        err,
      );
    } finally {
      clearTimeout(timer);
    }

    if (!response.ok) {
      throw new FlashStatError(
        `HTTP ${response.status} from FlashStat server`,
        response.status,
      );
    }

    const json = (await response.json()) as JsonRpcResponse<T>;
    if (isError(json)) {
      throw new FlashStatError(
        json.error.message,
        json.error.code,
        json.error.data,
      );
    }

    return json.result;
  }

  // ── Request / Response methods ─────────────────────────────────────────────

  /**
   * Returns the confidence score (`0.0`–`100.0`) for the given block hash.
   * Returns `0.0` for unknown blocks.
   */
  getConfidence(hash: string): Promise<number> {
    return this.rpc<number>("flash_getConfidence", [hash]);
  }

  /** Returns the most recently processed block, or `null` if none seen yet. */
  getLatestBlock(): Promise<FlashBlock | null> {
    return this.rpc<FlashBlock | null>("flash_getLatestBlock", []);
  }

  /** Returns the `limit` most recently processed blocks, newest first. */
  getRecentBlocks(limit: number): Promise<FlashBlock[]> {
    return this.rpc<FlashBlock[]>("flash_getRecentBlocks", [limit]);
  }

  /** Returns the `limit` most recent reorg or equivocation events. */
  getRecentReorgs(limit: number): Promise<ReorgEvent[]> {
    return this.rpc<ReorgEvent[]>("flash_getRecentReorgs", [limit]);
  }

  /** Returns only `Equivocation`-severity events (sequencer double-signing). */
  getEquivocations(limit: number): Promise<ReorgEvent[]> {
    return this.rpc<ReorgEvent[]>("flash_getEquivocations", [limit]);
  }

  /** Returns the current system health snapshot of the FlashStat node. */
  getHealth(): Promise<import("./types.js").SystemHealth> {
    return this.rpc("flash_getHealth", []);
  }

  /** Returns all tracked sequencer addresses ranked by reputation score. */
  getSequencerRankings(): Promise<import("./types.js").SequencerStats[]> {
    return this.rpc("flash_getSequencerRankings", []);
  }

  // ── Subscriptions ──────────────────────────────────────────────────────────

  /**
   * Streams a {@link FlashBlock} for every block processed by the monitor.
   *
   * @returns A function to call when you want to stop receiving updates.
   *
   * @example
   * ```typescript
   * const unsub = client.subscribeBlocks((block) => {
   *   if (block.confidence > 95) console.log('High-confidence block!', block.hash);
   * });
   * // Stop listening:
   * unsub();
   * ```
   */
  subscribeBlocks(callback: (block: FlashBlock) => void): UnsubscribeFn {
    return this.subscriptionManager.subscribe<FlashBlock>(
      "flash_subscribeBlocks",
      callback,
    );
  }

  /**
   * Streams a {@link ReorgEvent} whenever a soft reorg or equivocation is
   * detected by the watchtower.
   *
   * @returns A function to call when you want to stop receiving updates.
   */
  subscribeEvents(callback: (event: ReorgEvent) => void): UnsubscribeFn {
    return this.subscriptionManager.subscribe<ReorgEvent>(
      "flash_subscribeEvents",
      callback,
    );
  }

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  /** Closes all WebSocket connections and clears all subscriptions. */
  destroy(): void {
    this.subscriptionManager.destroy();
  }
}
