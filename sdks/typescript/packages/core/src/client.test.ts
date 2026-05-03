import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { FlashStatClient, FlashStatError } from "./client.js";
import type { SystemHealth, FlashBlock } from "./types.js";

// Mock the global fetch
const fetchMock = vi.fn();
global.fetch = fetchMock;

// Mock the global WebSocket
class MockWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  url: string;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: ((error: unknown) => void) | null = null;
  readyState: number = 0; // CONNECTING
  
  static instances: MockWebSocket[] = [];

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }

  send = vi.fn();
  close = vi.fn(() => {
    this.readyState = 3; // CLOSED
    if (this.onclose) this.onclose();
  });
  
  // Minimal EventTarget mock for addEventListener/removeEventListener
  listeners: Record<string, Function[]> = {};
  
  addEventListener(type: string, listener: Function) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(listener);
  }
  
  removeEventListener(type: string, listener: Function) {
    if (!this.listeners[type]) return;
    this.listeners[type] = this.listeners[type].filter(l => l !== listener);
  }
  
  // Test helpers
  triggerOpen() {
    this.readyState = 1; // OPEN
    if (this.onopen) this.onopen();
    if (this.listeners["open"]) this.listeners["open"].forEach(l => l());
  }
  
  triggerMessage(data: string) {
    const event = { data };
    if (this.onmessage) this.onmessage(event as any);
    if (this.listeners["message"]) this.listeners["message"].forEach(l => l(event));
  }
  
  triggerClose() {
    this.readyState = 3; // CLOSED
    if (this.onclose) this.onclose();
    if (this.listeners["close"]) this.listeners["close"].forEach(l => l());
  }
}

global.WebSocket = MockWebSocket as unknown as typeof WebSocket;

describe("FlashStatClient", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    MockWebSocket.instances = [];
  });

  afterEach(() => {
    //
  });

  describe("HTTP RPC Methods", () => {
    it("should successfully fetch system health", async () => {
      const mockHealth: SystemHealth = {
        uptimeSecs: 120,
        totalBlocks: 100,
        totalReorgs: 0,
        dbSizeBytes: 1024,
      };

      fetchMock.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          jsonrpc: "2.0",
          id: 1,
          result: mockHealth,
        }),
      });

      const client = new FlashStatClient({ url: "http://localhost:9944" });
      const health = await client.getHealth();

      expect(fetchMock).toHaveBeenCalledTimes(1);
      const call = fetchMock.mock.calls[0];
      expect(call[0]).toBe("http://localhost:9944");
      expect(JSON.parse(call[1].body)).toEqual({
        jsonrpc: "2.0",
        id: expect.any(Number),
        method: "flash_getHealth",
        params: [],
      });

      expect(health).toEqual(mockHealth);
    });

    it("should handle JSON-RPC errors correctly", async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({
          jsonrpc: "2.0",
          id: 1,
          error: { code: -32601, message: "Method not found" },
        }),
      });

      const client = new FlashStatClient({ url: "http://localhost:9944" });

      await expect(client.getHealth()).rejects.toThrowError(FlashStatError);
      await expect(client.getHealth()).rejects.toThrowError("Method not found");
    });
    
    it("should handle HTTP errors correctly", async () => {
      fetchMock.mockResolvedValueOnce({
        ok: false,
        status: 500,
        statusText: "Internal Server Error",
      });

      const client = new FlashStatClient({ url: "http://localhost:9944" });

      await expect(client.getHealth()).rejects.toThrowError("HTTP 500 from FlashStat server");
    });
  });

  describe("WebSocket Subscriptions", () => {
    it("should subscribe and receive block events", async () => {
      const client = new FlashStatClient({ url: "http://localhost:9944" });
      const callback = vi.fn();
      
      const unsub = client.subscribeBlocks(callback);
      
      // Client should have created a WebSocket
      expect(MockWebSocket.instances.length).toBe(1);
      const ws = MockWebSocket.instances[0];
      expect(ws.url).toBe("ws://localhost:9944");
      
      // Simulate socket open
      ws.triggerOpen();
      
      // Expect the subscribe RPC to be sent
      expect(ws.send).toHaveBeenCalledTimes(1);
      const subscribeReq = JSON.parse(ws.send.mock.calls[0][0]);
      expect(subscribeReq.method).toBe("flash_subscribeBlocks");
      
      // Simulate RPC success response containing subscription ID
      ws.triggerMessage(JSON.stringify({
        jsonrpc: "2.0",
        id: subscribeReq.id,
        result: "sub-123"
      }));
      
      // Simulate incoming block event
      const mockBlock: Partial<FlashBlock> = { hash: "0x123", number: "10" };
      ws.triggerMessage(JSON.stringify({
        jsonrpc: "2.0",
        method: "flash_subscribeBlocks",
        params: {
          subscription: "sub-123",
          result: mockBlock
        }
      }));
      
      expect(callback).toHaveBeenCalledWith(mockBlock);
      // Unsubscribe
      unsub();
      
      // Since client.ts implements unsubscribing by removing the callback locally
      // without storing subscription IDs to send an unsubscribe RPC, we just
      // verify the callback is no longer called.
      const mockBlock2: Partial<FlashBlock> = { hash: "0x456", number: "11" };
      ws.triggerMessage(JSON.stringify({
        jsonrpc: "2.0",
        method: "flash_subscribeBlocks",
        params: {
          subscription: "sub-123",
          result: mockBlock2
        }
      }));
      
      expect(callback).toHaveBeenCalledTimes(1); // Still 1, not 2
    });
  });
});
