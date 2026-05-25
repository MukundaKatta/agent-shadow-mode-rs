# agent-shadow-mode

Toggle shadow mode for agent tool calls. When active, records the intended call and returns a safe placeholder without running real side-effecting code.

## Usage

```rust
use agent_shadow_mode::ShadowMode;
use serde_json::json;

let mut shadow = ShadowMode::new(true); // start in shadow mode

let result = shadow.intercept(
    "charge_card",
    json!([]),
    json!({"customer_id": "cus_1", "amount_usd": 4.99}),
    json!({"status": "shadowed"}),
    || json!({"status": "charged"}), // real call — never runs when active
);
assert_eq!(result, json!({"status": "shadowed"}));

// Later: toggle off for real execution
shadow.deactivate();

// Persist the shadow log
shadow.to_jsonl("shadow-audit.jsonl").unwrap();
```

## License

MIT OR Apache-2.0
