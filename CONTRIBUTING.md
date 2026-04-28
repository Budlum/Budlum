# Contributing to Budlum Core

Thanks for helping improve Budlum Core.

Budlum is an experimental Rust Layer-1 blockchain core focused on modular consensus, deterministic execution, ZKVM-native contracts, privacy research, and future AI-assisted execution tooling. Contributions are welcome, especially from builders who enjoy protocol internals and clean systems code.

If you find the project useful, please also consider starring and forking the repo. It helps the project reach more protocol researchers and chain builders.

---

## Ways to Contribute

- Fix bugs in consensus, networking, storage, mempool, execution, or RPC code
- Improve tests, benchmarks, or chaos scenarios
- Add documentation for architecture and protocol behavior
- Review cryptography, finality, privacy, and VM design ideas
- Propose privacy-layer or private-VM experiments
- Improve developer experience and node operator tooling
- Open design discussions before large protocol changes

---

## Before You Start

1. Check existing issues and discussions.
2. Open an issue for large changes before writing a big patch.
3. Keep pull requests focused.
4. Prefer small, reviewable changes over one large rewrite.
5. Do not include secrets, private keys, validator credentials, or real production configs.

For security-sensitive findings, do not open a public issue. See [`SECURITY.md`](SECURITY.md).

---

## Development Setup

### Requirements

- Rust `1.70+`
- `protoc`
- Optional: Nix, if you use the provided development shell

### Build

```bash
cargo build
```

### Run Tests

```bash
cargo test
```

With Nix:

```bash
nix develop --command cargo test
```

### Format and Lint

```bash
cargo fmt
cargo clippy
```

Please run formatting before opening a pull request.

---

## Pull Request Guidelines

Good pull requests usually include:

- A clear description of the problem and the fix
- Tests for behavioral changes
- Documentation updates when public behavior changes
- Notes about consensus, storage, networking, or replay implications
- A short explanation of any tradeoffs

Avoid:

- Unrelated refactors mixed into feature work
- Formatting-only changes across unrelated files
- Changing protocol behavior without tests or explanation
- Adding dependencies unless they are clearly justified
- Introducing non-deterministic behavior into consensus or execution paths

---

## Consensus and Execution Changes

Changes in these areas need extra care:

- `src/consensus/`
- `src/execution/`
- `src/chain/`
- `src/core/transaction.rs`
- `src/core/block.rs`
- `src/storage/`
- `src/network/protocol.rs`
- `proto/protocol.proto`

Before changing protocol-critical logic, consider:

- Does replay produce the same result?
- Does reorg recovery stay deterministic?
- Does this change block, transaction, or state-root compatibility?
- Does the change require a schema or protocol version bump?
- Can malformed input trigger panic, resource exhaustion, or invalid state?
- Are tests covering both valid and invalid paths?

---

## Privacy and AI Roadmap Contributions

Budlum welcomes research-oriented proposals for:

- Shielded transaction designs
- Selective disclosure
- Private/custom VM execution
- Zero-knowledge proof integration
- Privacy-aware mempool behavior
- AI-assisted transaction simulation
- AI-assisted monitoring and anomaly detection
- Operator backend and analytics services

These features should remain optional and must not compromise deterministic consensus behavior.

---

## Commit Style

Use concise, descriptive commit messages.

Examples:

```text
fix: reject malformed contract bytecode
test: add mempool nonce queue regression case
docs: explain snapshot sync flow
feat: add devnet validator config option
```

---

## Community Expectations

Be direct, respectful, and technical. Strong disagreement is fine; personal attacks are not.

Assume contributors are here to make the protocol better. Ask questions, explain tradeoffs, and keep discussions grounded in code, tests, and reproducible behavior.

---

## License

By contributing to Budlum Core, you agree that your contributions will be licensed under the MIT License.
