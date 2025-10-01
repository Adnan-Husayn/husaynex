# husaynex

A learning project: a simple, educational blockchain implemented in Rust. husaynex is not a cryptocurrency - it's a compact, well‑documented implementation of core blockchain concepts so you can experiment with hashing, serialization, proof‑of‑work mining, validation, persistence, and (optionally) networking.

Built for learning and experimentation by Adnan Husayn.

---

## What this project teaches
- Cryptographic hashing (blake3 / sha2)
- Deterministic block hashing and block structure design
- Proof‑of‑Work mining (nonce search with adjustable difficulty)
- Chain validation (index continuity, prev_hash, hash recomputation, PoW check)
- Serialization (serde + serde_json) and persistent storage (JSON file)
- CLI tooling for common operations (mine, show, validate, import, export)

---

## Features (implemented)
- Block struct with: index, timestamp, data, nonce, prev_hash, hash
- Deterministic hashing via blake3
- Simple proof-of-work mining with adjustable difficulty
- Blockchain container with genesis block, add_block, and chain validation
- Serialize/deserialize chain to/from JSON file (persistence)
- CLI commands: mine, show, validate, export, import
- Unit tests for core properties (hash stability, mining, validation)

---

## Tech stack / crates
- Rust (2021/2024 edition)
- blake3 — hashing
- serde + serde_json — serialization
- clap — command-line interface
- chrono — timestamps
- anyhow — error handling

---

## Repository layout
- Cargo.toml
- src/
  - main.rs       — CLI entry
  - block.rs      — Block struct, hashing, mining
  - blockchain.rs — Blockchain struct, validation
  - storage.rs    — save/load JSON chain
  - cli.rs        — clap definitions (if split)
  - network.rs    — optional P2P (if added)
- data/
  - husaynex-chain.json    — saved chain (created at runtime)
- README.md

---

## Quickstart

1. Clone
```
git clone https://github.com/<your-username>/husaynex.git
cd husaynex
```

2. Build
```
cargo build --release
```

3. Run CLI
- Show help:
```
cargo run -- --help
```

- Mine a new block (example — difficulty currently taken from code or a config/flag)
```
cargo run -- mine --data "Hello, husaynex!"
```

- Show chain
```
cargo run -- show
```

- Validate chain
```
cargo run -- validate
```

- Export chain to file
```
cargo run -- export --path ./data/chain_export.json
```

- Import chain from file
```
cargo run -- import --path ./data/chain_export.json
```

Notes:
- By default the project saves/loads a JSON file (e.g., `data/chain.json`) — see storage config in `storage.rs` or main.
- Difficulty is adjustable via the blockchain initialization (change the difficulty parameter or expose it as a CLI flag/config).

---

## Minimal usage examples

Create and mine a block (concept)
```
let mut block = Block::new(next_index, "my data".into(), last_block.hash.clone());
block.mine(difficulty);
blockchain.add_block(block)?;
```

Validate chain
```
if blockchain.is_valid() {
    println!("chain valid");
} else {
    println!("chain invalid — tampering detected");
}
```

Save/load
```
save_chain("./data/chain.json", &blockchain)?;
let loaded = load_chain("./data/chain.json")?;
assert!(loaded.is_valid());
```

---

## Roadmap / Next steps
- Short-term
  - Make difficulty configurable from CLI or a config file
  - Improve mining performance (byte-level leading-zero check vs hex string)
  - Add JSON RPC / HTTP API for easier automation (warp / axum)
- Medium-term
  - Add transaction list instead of a single data string
  - Merkle root computation for transactions
  - Use binary storage (bincode) or embedded DB (sled / rocksdb)
- Optional / Advanced
  - Basic P2P sync using tokio TCP, exchange and adopt the longest valid chain
  - Add multi-threaded mining worker
  - Add benchmarks (criterion) for mining speed / scaling
  - Dockerfile for reproducible runs

---

## Design decisions & notes
- The persisted block hash is validated on load — do not trust stored data without validation.
- Timestamps use chrono (RFC3339) for readability; switch to epoch seconds if you prefer.
- Current PoW uses an n-hex-zero prefix check; for more accurate difficulty semantics, use leading zero bits (byte-level checks).
- The project is intentionally small and educational. Do not use it for production or as a real cryptocurrency.

---

## Contributing
Contributions, issues, and suggestions are welcome. If you want to add features or fixes:
- Fork the repo
- Create a feature branch
- Open a PR with description, tests, and rationale

Please keep changes small and test-covered when possible.

---

## License
Suggested: MIT License.

---

## Acknowledgements
This project is a personal educational tool inspired by many minimal blockchain tutorials and implementations. Thanks to the Rust community and the maintainers of blake3, serde, and clap.

---
