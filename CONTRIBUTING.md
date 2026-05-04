# Contributing to FlashStat

First off, thank you for considering contributing to FlashStat! It's people like you that make FlashStat a great tool for the Unichain ecosystem.

## 🏗 Project Overview

FlashStat is a monorepo consisting of:
- **Rust Backend**: The core monitoring engine, JSON-RPC server, and TEE verifier.
- **TypeScript SDKs**: Client libraries for Core, Viem, and React integration.

## 🛠 Development Setup

### Prerequisites
- **Rust**: Latest stable (edition 2024).
- **Node.js**: v18 or later.
- **pnpm**: v9 or later (used for SDK workspace management).
- **Docker**: For running the infrastructure locally.

### Repository Setup
```bash
git clone https://github.com/One-Block-Org/FlashStat.git
cd FlashStat

# Install SDK dependencies
cd sdks/typescript
pnpm install
```

## 🧪 Testing

We value high-quality tests. Before submitting a PR, please ensure all tests pass.

### Rust Backend Tests
```bash
cargo test --all-features
```

### TypeScript SDK Tests
```bash
cd sdks/typescript
pnpm test
```

## 📜 Coding Standards

### Rust
- Run `cargo fmt --all` before committing.
- Ensure `cargo clippy --all-targets --all-features` has no warnings.

### TypeScript
- We use Prettier for formatting.
- Ensure your changes are fully typed (no `any` unless absolutely necessary).

## 🚀 Pull Request Process

1.  **Fork the repo** and create your branch from `main`.
2.  **Make your changes**. If you've added code that should be tested, add tests.
3.  **Update documentation** if you've changed APIs or added features.
4.  **Submit a PR**. Describe your changes clearly and link to any related issues.

## 💬 Community

If you have questions or want to discuss a feature, feel free to open a [GitHub Issue](https://github.com/One-Block-Org/FlashStat/issues).

---

Thank you for helping us make Unichain safer!
