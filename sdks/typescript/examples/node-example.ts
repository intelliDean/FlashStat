/**
 * FlashStat SDK — Node.js / Vanilla JS Example
 *
 * Demonstrates all core @flashstat/core features:
 *   1. Querying block confidence via HTTP RPC
 *   2. Subscribing to live block and reorg event streams
 *   3. Fetching the sequencer reputation leaderboard
 *   4. Reading system health
 *
 * Prerequisites:
 *   - A running flashstat-server: `cargo run --release --bin flashstat-server`
 *   - Node.js 18+ (for native fetch + WebSocket)
 *
 * Run this file with:
 *   npx tsx node-example.ts
 *   or
 *   node --experimental-strip-types node-example.ts
 */

import { FlashStatClient, FlashStatError } from "@flashstat/core";
import type { FlashBlock, ReorgEvent } from "@flashstat/core";

// ─── Config ───────────────────────────────────────────────────────────────────

const FLASHSTAT_URL = process.env["FLASHSTAT_URL"] ?? "http://127.0.0.1:9944";

// ─── Helpers ──────────────────────────────────────────────────────────────────

function confidenceBar(score: number): string {
  const filled = Math.round(score / 5); // 20 chars = 100%
  const bar = "█".repeat(filled) + "░".repeat(20 - filled);
  return `[${bar}] ${score.toFixed(1)}%`;
}

function statusEmoji(status: FlashBlock["status"]): string {
  return { Stable: "🟢", Pending: "🟡", Finalized: "✅", Reorged: "🔴" }[status] ?? "⚪";
}

// ─── Main ─────────────────────────────────────────────────────────────────────

async function main() {
  console.log(`\n⚡ FlashStat SDK — Node.js Example`);
  console.log(`   Connecting to: ${FLASHSTAT_URL}\n`);

  const client = new FlashStatClient({ url: FLASHSTAT_URL });

  // ── 1. System Health ───────────────────────────────────────────────────────
  console.log("── System Health ─────────────────────────────────────────────");
  try {
    const health = await client.getHealth();
    console.log(`  Uptime:       ${Math.floor(health.uptimeSecs / 60)}m ${health.uptimeSecs % 60}s`);
    console.log(`  Total blocks: ${health.totalBlocks.toLocaleString()}`);
    console.log(`  Total reorgs: ${health.totalReorgs}`);
    console.log(`  DB size:      ${(health.dbSizeBytes / 1024).toFixed(1)} KB`);
  } catch (err) {
    if (err instanceof FlashStatError) {
      console.error(`  ✗ Health check failed [${err.code}]: ${err.message}`);
    }
  }

  // ── 2. Latest Block ────────────────────────────────────────────────────────
  console.log("\n── Latest Block ──────────────────────────────────────────────");
  try {
    const block = await client.getLatestBlock();
    if (block) {
      console.log(`  Number:     #${BigInt(block.number).toLocaleString()}`);
      console.log(`  Hash:       ${block.hash}`);
      console.log(`  Signer:     ${block.signer ?? "(none)"}`);
      console.log(`  Status:     ${statusEmoji(block.status)} ${block.status}`);
      console.log(`  Confidence: ${confidenceBar(block.confidence)}`);
    } else {
      console.log("  No blocks ingested yet.");
    }
  } catch (err) {
    console.error("  ✗ Failed to fetch latest block:", (err as Error).message);
  }

  // ── 3. Specific Block Confidence ───────────────────────────────────────────
  console.log("\n── Block Confidence (by hash) ────────────────────────────────");
  // In a real integration you'd pass the actual tx/block hash here.
  const exampleHash = "0x0000000000000000000000000000000000000000000000000000000000000001";
  try {
    const confidence = await client.getConfidence(exampleHash);
    console.log(`  Hash:       ${exampleHash}`);
    console.log(`  Confidence: ${confidenceBar(confidence)}`);
    if (confidence > 95) {
      console.log("  ✓ SAFE — Confidence above threshold. Proceed with action.");
    } else if (confidence > 70) {
      console.log("  ⚠ CAUTION — Confidence moderate. Consider waiting.");
    } else {
      console.log("  ✗ UNSAFE — Confidence too low. Do not act on this block.");
    }
  } catch (err) {
    console.log("  (unknown block — 0.0% returned for untracked hashes)");
  }

  // ── 4. Recent Reorgs ───────────────────────────────────────────────────────
  console.log("\n── Recent Reorg Events ───────────────────────────────────────");
  try {
    const reorgs = await client.getRecentReorgs(5);
    if (reorgs.length === 0) {
      console.log("  ✓ No recent reorg events detected.");
    } else {
      for (const r of reorgs) {
        const icon = r.severity === "Equivocation" ? "🚨" : "⚠";
        console.log(`  ${icon} [${r.severity}] Block #${BigInt(r.blockNumber).toLocaleString()}`);
        console.log(`     Old: ${r.oldHash}`);
        console.log(`     New: ${r.newHash}`);
        if (r.equivocation) {
          console.log(`     Signer: ${r.equivocation.signer}`);
        }
      }
    }
  } catch (err) {
    console.error("  ✗ Failed to fetch reorg events:", (err as Error).message);
  }

  // ── 5. Sequencer Reputation Leaderboard ───────────────────────────────────
  console.log("\n── Sequencer Leaderboard ─────────────────────────────────────");
  try {
    const rankings = await client.getSequencerRankings();
    if (rankings.length === 0) {
      console.log("  No sequencers tracked yet.");
    } else {
      for (const [i, s] of rankings.entries()) {
        const medal = ["🥇", "🥈", "🥉"][i] ?? `${i + 1}.`;
        const score = s.reputationScore >= 0
          ? `+${s.reputationScore.toLocaleString()}`
          : s.reputationScore.toLocaleString();
        console.log(`  ${medal} ${s.address}`);
        console.log(`     Score: ${score} | Blocks: ${s.totalBlocksSigned} | Streak: ${s.currentStreak}`);
        if (s.totalEquivocations > 0) {
          console.log(`     🚨 SLASHABLE — ${s.totalEquivocations} equivocation(s) detected!`);
        }
      }
    }
  } catch (err) {
    console.error("  ✗ Failed to fetch rankings:", (err as Error).message);
  }

  // ── 6. Live Subscriptions (30-second demo) ────────────────────────────────
  console.log("\n── Live Subscriptions (30s demo) ─────────────────────────────");
  console.log("  Subscribing to block and reorg streams…\n");

  let blockCount = 0;
  let reorgCount = 0;

  const unsubBlocks = client.subscribeBlocks((block: FlashBlock) => {
    blockCount++;
    console.log(
      `  📦 Block #${BigInt(block.number)} ${statusEmoji(block.status)} ${confidenceBar(block.confidence)}`
    );
  });

  const unsubEvents = client.subscribeEvents((event: ReorgEvent) => {
    reorgCount++;
    const icon = event.severity === "Equivocation" ? "🚨 EQUIVOCATION" : "⚠ REORG";
    console.log(`  ${icon} detected at block #${BigInt(event.blockNumber)}`);
  });

  await new Promise((resolve) => setTimeout(resolve, 30_000));

  unsubBlocks();
  unsubEvents();
  client.destroy();

  console.log(`\n── Summary ───────────────────────────────────────────────────`);
  console.log(`  Blocks received:  ${blockCount}`);
  console.log(`  Reorgs detected:  ${reorgCount}`);
  console.log("\n  Done. ✓\n");
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
