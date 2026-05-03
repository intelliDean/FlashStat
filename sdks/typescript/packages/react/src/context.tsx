import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  type ReactNode,
} from "react";
import { FlashStatClient, type FlashStatClientConfig } from "@flashstat/core";

// ─── Context ──────────────────────────────────────────────────────────────────

const FlashStatContext = createContext<FlashStatClient | null>(null);

// ─── Provider ─────────────────────────────────────────────────────────────────

export interface FlashStatProviderProps {
  /** FlashStat node URL (e.g. `"http://127.0.0.1:9944"`). */
  url: string;
  /** Optional WebSocket URL for subscriptions. Defaults to URL with `http→ws`. */
  wsUrl?: string;
  /** Request timeout in milliseconds. Defaults to `5000`. */
  timeoutMs?: number;
  /** Auto-reconnect delay in milliseconds. Defaults to `2000`. */
  reconnectMs?: number;
  children: ReactNode;
}

/**
 * Provides a single shared {@link FlashStatClient} to all descendant hooks.
 * Place this near the root of your application.
 *
 * @example
 * ```tsx
 * import { FlashStatProvider } from '@flashstat/react';
 *
 * function App() {
 *   return (
 *     <FlashStatProvider url="http://127.0.0.1:9944">
 *       <YourApp />
 *     </FlashStatProvider>
 *   );
 * }
 * ```
 */
export function FlashStatProvider({
  url,
  wsUrl,
  timeoutMs,
  reconnectMs,
  children,
}: FlashStatProviderProps) {
  const config: FlashStatClientConfig = useMemo(
    () => ({
      url,
      ...(wsUrl !== undefined && { wsUrl }),
      ...(timeoutMs !== undefined && { timeoutMs }),
      ...(reconnectMs !== undefined && { reconnectMs }),
    }),
    [url, wsUrl, timeoutMs, reconnectMs],
  );

  const client = useMemo(() => new FlashStatClient(config), [config]);

  // Tear down the WebSocket connection when the provider unmounts.
  useEffect(() => () => client.destroy(), [client]);

  return (
    <FlashStatContext.Provider value={client}>
      {children}
    </FlashStatContext.Provider>
  );
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

/**
 * Returns the nearest ancestor {@link FlashStatClient} provided by
 * {@link FlashStatProvider}. Throws if used outside of a provider.
 */
export function useFlashStatClient(): FlashStatClient {
  const client = useContext(FlashStatContext);
  if (!client) {
    throw new Error(
      "useFlashStatClient must be used inside a <FlashStatProvider>.",
    );
  }
  return client;
}
