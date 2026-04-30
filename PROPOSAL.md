# Unichain Grant Proposal: FlashStat

## 1. Project Overview

**Project Name:** FlashStat
**Category:** Core Infrastructure & Security / Developer Tooling
**Repository:** [Link to your GitHub repository]

### Executive Summary
FlashStat is the trust layer for Unichain’s soft-finality. It is an independent, real-time cryptographic watchtower and reputation engine designed specifically for Unichain's 200ms Flashblocks. FlashStat ingests pre-confirmations in real-time, validates Intel TDX hardware attestations, and exposes a mathematical "confidence score" via a public JSON-RPC API. In the event of sequencer misbehavior (equivocations or soft reorgs), FlashStat autonomously generates cryptographic fraud proofs and submits them on-chain for active slashing.

By providing quantifiable risk metrics for 200ms blocks, FlashStat enables high-value actors—such as market makers, liquidators, and cross-chain bridges—to trust Unichain's pre-confirmations without waiting for Ethereum L1 finality.

## 2. The Problem: The Fragility of Soft-Finality

Unichain’s primary competitive advantage is its incredible speed, achieved through 200-millisecond Flashblocks. However, this speed introduces a critical trust problem. 

Until a block is finalized on Ethereum L1, users and market makers must blindly trust that the sequencer will not reorg the chain, double-spend transactions, or extract malicious MEV. If high-value ecosystem participants cannot mathematically trust these pre-confirmations, they will default to waiting for hard finality. 

**If users wait for L1 finality to manage risk, Unichain loses its speed advantage.**

While Unichain mitigates this via Trusted Execution Environments (TEEs like Intel TDX), a TEE is only secure if there is an active, decentralized network of watchtowers validating the attestations and punishing deviations. Without an active watchtower network and an accessible API for developers to verify block confidence, the TEE security model is incomplete.

## 3. The Solution: FlashStat

FlashStat bridges the gap between 200ms pre-confirmations and L1 finality. It operates as a high-performance Rust monorepo with three core functions:

1. **The Cryptographic Watchtower (Detection & Slashing):** 
   FlashStat sits adjacent to the Unichain sequencer, recording every finalized bid. It validates the ECDSA signatures and Intel TDX Quote V4 attestations appended to the `extra_data` of every block. If a sequencer equivocates (signs two conflicting blocks at the same height), FlashStat instantly detects the conflict, diffs the transactions to identify double-spends, encodes an RLP fraud proof, and autonomously submits it to the SlashingManager contract via its Guardian Wallet.

2. **The Confidence API (Developer Tooling):**
   FlashStat provides a JSON-RPC and WebSocket API that assigns a live `0.0` to `100.0` confidence score to every Flashblock. This score is calculated by combining historical persistence depth with TEE attestation validity. Developers can query `flash_getConfidence` to programmatically gate UI updates or smart contract interactions based on quantifiable risk.

3. **The Reputation Engine (Ecosystem Transparency):**
   FlashStat maintains a persistent, public leaderboard of sequencer health. It tracks total blocks signed, hardware-attested streaks, soft reorgs, and equivocations. Penalties are weighted mathematically, turning sequencer reliability into a transparent, quantifiable metric.

## 4. Why Unichain Needs FlashStat

* **Unlocks Institutional Adoption:** DeFi protocols, market makers, and bridges require quantifiable risk models. FlashStat provides the API necessary for these actors to safely treat 200ms pre-confirmations as final.
* **Enforces TEE Security:** The Intel TDX hardware lock is only effective if malicious attestations result in economic penalties. FlashStat is the active enforcement mechanism that keeps sequencers honest.
* **Enhances Developer Experience:** Instead of every dApp building complex logic to track soft-reorgs and signature validation, they can simply subscribe to FlashStat's WebSocket stream to receive cleansed, scored block events.

## 5. Current State of the Project

The core FlashStat engine is built, fully functional, and production-ready. The codebase is a high-performance Rust workspace consisting of:
*   **100% Test Coverage:** 70/70 passing unit and integration tests.
*   **Core Engine (`flashstat-core`):** Real-time block ingestion, TDX/TEE attestation verification, and RLP proof encoding.
*   **Embedded Storage (`flashstat-db`):** High-speed `redb` persistence layer.
*   **API Server (`flashstat-server`):** Fully documented JSON-RPC 2.0 and WebSocket server.
*   **Dashboard (`flashstat-tui`):** A live terminal UI for real-time forensics.

## 6. Grant Ask & Roadmap

We are seeking grant funding to transition FlashStat from a working, open-source core engine into a **highly available public good** for the Unichain ecosystem. 

**Funding will be allocated to achieve the following milestones:**

### Milestone 1: Public Infrastructure Deployment (1 Month)
*   Deploy globally distributed, highly available FlashStat instances monitoring the Unichain Mainnet and Sepolia testnets.
*   Provide free, public JSON-RPC and WebSocket endpoints for Unichain developers to query block confidence and reorg events.
*   Set up robust telemetry (Prometheus/Grafana) and alerting for the watchtower nodes.

### Milestone 2: Developer SDKs and Integrations (2 Months)
*   Develop and release a `flashstat-sdk` in TypeScript and Rust.
*   Build drop-in React hooks (`useFlashblockConfidence`) so dApp frontend developers can easily show users real-time transaction finality (e.g., updating a UI spinner to a green checkmark when confidence hits 99%).
*   Create comprehensive integration documentation and tutorials for Unichain developers.

### Milestone 3: Decentralized Guardian Network (3 Months)
*   Expand the current single-node Guardian Wallet architecture into a multi-party computation (MPC) or threshold signature scheme (TSS) model.
*   Enable community members to run lightweight FlashStat verifier nodes that collectively vote on and submit slashing proofs, eliminating any single point of failure in the watchtower network.

## 7. Team

[Insert brief team bios here, highlighting Rust expertise, blockchain infrastructure experience, and previous contributions.]

## 8. Conclusion

FlashStat ensures that Unichain's 200ms speed is matched by ironclad, cryptographic trust. By funding the public deployment and SDK development of FlashStat, Unichain will provide its developers with the tooling necessary to safely build the next generation of high-frequency DeFi.
