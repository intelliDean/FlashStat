"use client";

import {
  useFlashHealth,
  useLatestFlashBlock,
  useReorgEvents,
  useSequencerRankings,
} from "@flashstat/react";
import type { FlashBlock, ReorgEvent, SequencerStats } from "@flashstat/react";

// ─── Utility helpers ──────────────────────────────────────────────────────────

function shortHash(hash: string): string {
  return `${hash.slice(0, 8)}…${hash.slice(-6)}`;
}

function confidenceClass(score: number): "high" | "medium" | "low" {
  if (score >= 90) return "high";
  if (score >= 60) return "medium";
  return "low";
}

function medal(i: number): string {
  return ["🥇", "🥈", "🥉"][i] ?? `${i + 1}.`;
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: FlashBlock["status"] }) {
  const cls: Record<FlashBlock["status"], string> = {
    Stable:    "badge badge-stable",
    Pending:   "badge badge-pending",
    Finalized: "badge badge-finalized",
    Reorged:   "badge badge-reorged",
  };
  return (
    <span className={cls[status]}>
      <span className="badge-dot" />
      {status}
    </span>
  );
}

function HealthCard() {
  const { health, isLoading, error } = useFlashHealth(30_000);

  return (
    <div className="card">
      <p className="card-title">System Health</p>
      {isLoading && <div className="loading"><div className="spinner" />Loading…</div>}
      {error && <p className="error-text">⚠ {error.message}</p>}
      {health && (
        <div className="stat-grid">
          <div className="stat">
            <span className="stat-value">
              {Math.floor(health.uptimeSecs / 60)}m {health.uptimeSecs % 60}s
            </span>
            <span className="stat-label">Uptime</span>
          </div>
          <div className="stat">
            <span className="stat-value">{health.totalBlocks.toLocaleString()}</span>
            <span className="stat-label">Blocks Processed</span>
          </div>
          <div className="stat">
            <span className="stat-value" style={{ color: health.totalReorgs > 0 ? "var(--orange)" : "var(--green)" }}>
              {health.totalReorgs}
            </span>
            <span className="stat-label">Reorgs Detected</span>
          </div>
          <div className="stat">
            <span className="stat-value">{(health.dbSizeBytes / 1024).toFixed(1)} KB</span>
            <span className="stat-label">Database Size</span>
          </div>
        </div>
      )}
    </div>
  );
}

function LatestBlockCard() {
  const { block, isLoading, error } = useLatestFlashBlock();
  const cls = block ? confidenceClass(block.confidence) : "low";

  return (
    <div className="card">
      <p className="card-title">Latest Flashblock</p>
      {isLoading && <div className="loading"><div className="spinner" />Connecting…</div>}
      {error && <p className="error-text">⚠ {error.message}</p>}
      {block && (
        <div className="confidence-wrap">
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
            <div>
              <div className={`confidence-score ${cls}`}>
                {block.confidence.toFixed(1)}%
              </div>
              <div className="confidence-label">
                Block #{BigInt(block.number).toLocaleString()} confidence
              </div>
            </div>
            <StatusBadge status={block.status} />
          </div>
          <div className="confidence-track">
            <div
              className={`confidence-fill ${cls}`}
              style={{ width: `${block.confidence}%` }}
            />
          </div>
          {block.signer && (
            <div className="confidence-label mono">
              Signer: {shortHash(block.signer)}
            </div>
          )}
        </div>
      )}
      {!block && !isLoading && !error && (
        <p style={{ color: "var(--muted)", fontSize: 13 }}>No blocks yet.</p>
      )}
    </div>
  );
}

function BlockFeed() {
  // We reuse the latest block hook but accumulate a local feed
  const [feed, setFeed] = React.useState<FlashBlock[]>([]);
  const { block } = useLatestFlashBlock();

  React.useEffect(() => {
    if (!block) return;
    setFeed((prev) => {
      if (prev[0]?.hash === block.hash) return prev;
      return [block, ...prev].slice(0, 20);
    });
  }, [block]);

  return (
    <div className="card grid-wide">
      <p className="card-title">Live Block Feed</p>
      <div className="block-feed">
        {feed.length === 0 && (
          <div className="loading"><div className="spinner" />Waiting for blocks…</div>
        )}
        {feed.map((b) => (
          <div key={b.hash} className="block-row">
            <span className="block-number">#{BigInt(b.number).toLocaleString()}</span>
            <span className="block-hash">{shortHash(b.hash)}</span>
            <div className="block-mini-bar">
              <div className="block-mini-fill" style={{ width: `${b.confidence}%` }} />
            </div>
            <StatusBadge status={b.status} />
          </div>
        ))}
      </div>
    </div>
  );
}

function ReorgLog() {
  const { events, isLoading } = useReorgEvents(10);

  return (
    <div className="card">
      <p className="card-title">Reorg Log</p>
      {isLoading && <div className="loading"><div className="spinner" />Loading…</div>}
      <div className="reorg-list">
        {events.length === 0 && !isLoading && (
          <div className="reorg-empty">
            <span>✅</span> No reorg events detected.
          </div>
        )}
        {events.map((r) => (
          <div
            key={`${r.blockNumber}-${r.detectedAt}`}
            className={`reorg-row ${r.severity === "Equivocation" ? "equivocation" : ""}`}
          >
            <div className="reorg-row-top">
              <span className="reorg-severity">
                {r.severity === "Equivocation" ? "🚨 " : "⚠ "}{r.severity}
              </span>
              <span className="reorg-block">Block #{BigInt(r.blockNumber).toLocaleString()}</span>
            </div>
            <div className="reorg-hashes">
              Old: {shortHash(r.oldHash)}<br />
              New: {shortHash(r.newHash)}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function Leaderboard() {
  const { rankings, isLoading, refresh } = useSequencerRankings(10_000);

  return (
    <div className="card">
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 }}>
        <p className="card-title" style={{ margin: 0 }}>Sequencer Leaderboard</p>
        <button
          onClick={refresh}
          style={{
            background: "var(--surface-2)", border: "1px solid var(--border)",
            color: "var(--muted)", fontSize: 11, padding: "3px 10px",
            borderRadius: 6, cursor: "pointer"
          }}
        >
          ↻ Refresh
        </button>
      </div>
      {isLoading && <div className="loading"><div className="spinner" />Loading…</div>}
      <div className="leaderboard">
        {rankings.length === 0 && !isLoading && (
          <p style={{ color: "var(--muted)", fontSize: 13 }}>No sequencers tracked yet.</p>
        )}
        {rankings.map((s: SequencerStats, i: number) => (
          <div key={s.address} className="leader-row">
            <span className="leader-rank">{medal(i)}</span>
            <div style={{ flex: 1 }}>
              <div className="leader-addr">{shortHash(s.address)}</div>
              <div className="leader-meta">
                {s.totalBlocksSigned} blocks · streak: {s.currentStreak}
              </div>
              {s.totalEquivocations > 0 && (
                <div className="leader-slash">
                  🚨 {s.totalEquivocations} equivocation(s)
                </div>
              )}
            </div>
            <span className={`leader-score ${s.reputationScore < 0 ? "negative" : ""}`}>
              {s.reputationScore >= 0 ? "+" : ""}{s.reputationScore.toLocaleString()}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ─── Reorg Alert Banner ───────────────────────────────────────────────────────

function ReorgAlertBanner() {
  const { events } = useReorgEvents(1);
  const latest = events[0];

  if (!latest || latest.severity !== "Equivocation") return null;

  return (
    <div className="alert-banner">
      🚨 <strong>EQUIVOCATION DETECTED</strong> — Sequencer double-signed at block{" "}
      #{BigInt(latest.blockNumber).toLocaleString()}. Fraud proof submitted.
    </div>
  );
}

// ─── Page ─────────────────────────────────────────────────────────────────────

import React from "react";
import { Providers } from "./providers";

export default function DashboardPage() {
  return (
    <Providers>
      <div className="app">
        <header className="header">
          <span className="header-logo">⚡</span>
          <div>
            <h1>FlashStat Dashboard</h1>
            <p>Real-time Unichain Flashblock confidence & sequencer reputation</p>
          </div>
        </header>

        <ReorgAlertBanner />

        <div className="grid" style={{ marginBottom: 16 }}>
          <HealthCard />
          <LatestBlockCard />
        </div>

        <BlockFeed />

        <div className="grid" style={{ marginTop: 16 }}>
          <ReorgLog />
          <Leaderboard />
        </div>
      </div>
    </Providers>
  );
}
