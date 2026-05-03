// ─── Enums ───────────────────────────────────────────────────────────────────

/** Maps to Rust `BlockStatus` enum in `flashstat-common`. */
export type BlockStatus = "Pending" | "Stable" | "Finalized" | "Reorged";

/** Maps to Rust `ReorgSeverity` enum in `flashstat-common`. */
export type ReorgSeverity = "Soft" | "Deep" | "Equivocation";

// ─── Core Types ───────────────────────────────────────────────────────────────

/**
 * A single Flashblock processed by the monitor.
 * Maps to Rust `FlashBlock` struct in `flashstat-common`.
 */
export interface FlashBlock {
  /** Block number as a hex string (e.g. "0x1a4"). */
  number: string;
  /** 0x-prefixed block hash. */
  hash: string;
  /** 0x-prefixed parent block hash. */
  parentHash: string;
  /** ISO-8601 timestamp string. */
  timestamp: string;
  /** Raw 65-byte ECDSA sequencer signature as a hex string, or null if absent. */
  sequencerSignature: string | null;
  /** Checksummed sequencer address recovered from the signature, or null. */
  signer: string | null;
  /** Confidence score from 0.0 (unknown) to 100.0 (fully attested). */
  confidence: number;
  /** Lifecycle status of this block. */
  status: BlockStatus;
}

/**
 * A detected chain reorganisation or equivocation event.
 * Maps to Rust `ReorgEvent` struct in `flashstat-common`.
 */
export interface ReorgEvent {
  /** Hex string block number where the conflict occurred. */
  blockNumber: string;
  /** The hash that was previously considered canonical. */
  oldHash: string;
  /** The conflicting new hash. */
  newHash: string;
  /** ISO-8601 timestamp of detection. */
  detectedAt: string;
  /** Severity classification. */
  severity: ReorgSeverity;
  /** Populated when severity is `Equivocation`. */
  equivocation: EquivocationEvent | null;
}

/**
 * Details of an equivocation — a sequencer signing two conflicting blocks.
 * Maps to Rust `EquivocationEvent` struct.
 */
export interface EquivocationEvent {
  /** Checksummed address of the misbehaving sequencer. */
  signer: string;
  /** Hex-encoded 65-byte ECDSA signature for the first block. */
  signature1: string;
  /** Hex-encoded 65-byte ECDSA signature for the conflicting block. */
  signature2: string;
  /** Transaction-level analysis of the conflict, if available. */
  conflictAnalysis: ConflictAnalysis | null;
}

/** Transaction diff between two conflicting blocks. */
export interface ConflictAnalysis {
  /** Tx hashes present in the first block but dropped in the second. */
  droppedTxs: string[];
  /** Transactions that appear to re-use the same nonce (double-spend). */
  doubleSpendTxs: DoubleSpendProof[];
}

/** A pair of transactions from the same sender sharing the same nonce. */
export interface DoubleSpendProof {
  txHash1: string;
  txHash2: string;
  /** Checksummed sender address. */
  sender: string;
  /** Shared nonce as a hex string. */
  nonce: string;
}

/**
 * Live health snapshot of the FlashStat node.
 * Maps to Rust `SystemHealth` struct.
 */
export interface SystemHealth {
  uptimeSecs: number;
  totalBlocks: number;
  totalReorgs: number;
  dbSizeBytes: number;
}

/**
 * Per-sequencer reputation record.
 * Maps to Rust `SequencerStats` struct.
 */
export interface SequencerStats {
  /** Checksummed sequencer address. */
  address: string;
  totalBlocksSigned: number;
  totalAttestedBlocks: number;
  totalSoftReorgs: number;
  totalEquivocations: number;
  currentStreak: number;
  /** Can be negative if equivocations have been detected. */
  reputationScore: number;
  /** ISO-8601 timestamp of the last seen block from this sequencer. */
  lastActive: string;
}

// ─── Utility ─────────────────────────────────────────────────────────────────

/** Call to stop a WebSocket subscription. */
export type UnsubscribeFn = () => void;
