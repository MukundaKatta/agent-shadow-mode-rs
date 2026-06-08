# agent-shadow-mode

[![CI](https://github.com/MukundaKatta/agent-shadow-mode-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MukundaKatta/agent-shadow-mode-rs/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A tiny, dependency-light **record-not-execute** wrapper for AI agent tool calls.

When *shadow mode* is active, a tool call is **recorded** to an in-memory audit
log but the underlying side-effecting function is **never run** — `execute`
returns `Ok(None)` instead. Flip shadow mode off and the same call runs for real
and its result is recorded. This lets you dry-run an agent against destructive
tools (charging a card, sending an email, deleting a row) and inspect exactly
what it *would* have done.

## Features

- Toggle shadow mode on/off at runtime (`set_shadow` / `is_shadow`).
- Every call is appended to an audit log: name, arguments, result, whether it
  was shadowed, and a Unix timestamp.
- Cheap to share: `ShadowAgent` is `Clone` and all clones share the same
  log and shadow flag (backed by `Arc<Mutex<…>>`).
- Export the audit log as [JSON Lines](https://jsonlines.org/) for persistence
  or downstream analysis (`to_jsonl` / `write_jsonl`).
- One small dependency (`serde_json`).

## Install

Add it to your `Cargo.toml`:

```toml
[dependencies]
agent-shadow-mode = "0.1"
serde_json = "1"
```

## Usage

```rust
use agent_shadow_mode::ShadowAgent;
use serde_json::json;

// Start in shadow mode: nothing destructive will actually run.
let agent = ShadowAgent::new(true);

let result = agent
    .execute("charge_card", &json!({"customer": "cus_1", "amount_usd": 4.99}), || {
        // This closure is the *real* call. In shadow mode it never runs.
        json!({"status": "charged"})
    })
    .unwrap();

assert!(result.is_none());        // shadowed: the closure did not run
assert!(agent.has_shadowed());    // ...but the intent was recorded

// Flip to live execution and run the same kind of call for real.
agent.set_shadow(false);
let result = agent
    .execute("lookup", &json!({"id": 7}), || json!({"name": "ada"}))
    .unwrap();
assert_eq!(result, Some(json!({"name": "ada"})));

// Persist the audit trail as JSON Lines.
agent.write_jsonl("shadow-audit.jsonl").unwrap();
```

Each line of the exported file is a self-contained JSON object, e.g.:

```jsonl
{"name":"charge_card","args":{"amount_usd":4.99,"customer":"cus_1"},"result":null,"was_shadow":true,"timestamp":1.7e9}
{"name":"lookup","args":{"id":7},"result":{"name":"ada"},"was_shadow":false,"timestamp":1.7e9}
```

## API

### `ShadowAgent`

| Method | Description |
| --- | --- |
| `ShadowAgent::new(shadow: bool)` | Create an agent. `shadow = true` suppresses execution. |
| `execute(name, args, f) -> Result<Option<Value>, String>` | Record a call. Returns `Ok(None)` when shadowed, `Ok(Some(value))` otherwise. `f` runs only when **not** shadowed. |
| `set_shadow(enabled: bool)` | Turn shadow mode on or off at runtime. |
| `is_shadow() -> bool` | Whether shadow mode is currently active. |
| `entries() -> Vec<AuditEntry>` | A snapshot of all recorded calls, in order. |
| `count(name: &str) -> usize` | Number of recorded calls with the given name. |
| `has_shadowed() -> bool` | Whether any recorded call was shadowed. |
| `clear()` | Empty the audit log. |
| `to_jsonl() -> String` | The audit log as JSON Lines (no trailing newline; empty log → `""`). |
| `write_jsonl(path) -> io::Result<()>` | Write the audit log to a file as JSON Lines. |

### `AuditEntry`

A single recorded invocation with fields `name`, `args`, `result`
(`Option<Value>`; `None` when shadowed), `was_shadow`, and `timestamp`.
`AuditEntry::to_value()` renders it as a JSON object (with `result` becoming
`null` when absent).

## Development

```sh
cargo build
cargo test            # unit + integration tests + doctests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option.
