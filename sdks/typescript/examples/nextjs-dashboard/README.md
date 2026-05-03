# FlashStat Next.js Dashboard Example

A minimal Next.js 14 app (App Router) that uses `@flashstat/react` to display
a live FlashStat dashboard directly in the browser.

## What it shows

- **Live block feed** — every new Flashblock appears with its confidence score and status badge
- **Reorg alert banner** — flashes red when an equivocation or reorg is detected
- **Sequencer leaderboard** — auto-refreshes every 10 seconds
- **System health** — uptime, total blocks, reorg count

## Running

```bash
# 1. Start the Rust server first
cargo run --release --bin flashstat-server

# 2. In another terminal, run the Next.js app
cd sdks/typescript/examples/nextjs-dashboard
npm install
npm run dev
```

Open http://localhost:3000

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `NEXT_PUBLIC_FLASHSTAT_URL` | `http://127.0.0.1:9944` | FlashStat server URL |

Set it in `.env.local` for production deployments.
