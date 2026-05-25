/*!
agent-shadow-mode: toggleable shadow mode for agent tool calls.

When rolling out a new agent, you don't want it to actually charge cards,
send emails, or delete files on day one. Wrap your tool calls through
`ShadowMode`: when active, it records the intended call and returns a safe
placeholder without running the real function.

```rust
use agent_shadow_mode::ShadowMode;
use serde_json::json;

let mut shadow = ShadowMode::new(true);

// Shadow mode active: real closure never runs
let result = shadow.intercept("charge_card", json!([]), json!({}), json!({"status": "shadowed"}), || {
    json!({"status": "charged"}) // would run the real call
});
assert_eq!(result, json!({"status": "shadowed"}));
assert_eq!(shadow.records().len(), 1);
assert_eq!(shadow.records()[0].tool_name, "charge_card");

// Deactivate: real closure runs
shadow.deactivate();
let real = shadow.intercept("charge_card", json!([]), json!({}), json!({"status": "shadowed"}), || {
    json!({"status": "charged"})
});
assert_eq!(real, json!({"status": "charged"}));
```
*/

use serde_json::{Map, Value};
use std::path::Path;

// ---- ShadowRecord ---------------------------------------------------------

/// One recorded would-be tool call.
#[derive(Debug, Clone)]
pub struct ShadowRecord {
    pub tool_name: String,
    /// Positional args passed to the tool (serialized as JSON array items).
    pub args: Vec<Value>,
    /// Keyword args passed to the tool.
    pub kwargs: Map<String, Value>,
    /// Unix timestamp when the call was intercepted.
    pub ts: f64,
    /// The value that was returned in shadow mode (the placeholder).
    pub returned: Value,
}

impl ShadowRecord {
    /// Serialize to a single-line JSON string for JSONL output.
    pub fn to_json_line(&self) -> String {
        let obj = serde_json::json!({
            "tool_name": self.tool_name,
            "args": self.args,
            "kwargs": self.kwargs,
            "ts": self.ts,
            "returned": self.returned,
        });
        serde_json::to_string(&obj).unwrap_or_default()
    }

    /// Deserialize from a JSONL line.
    pub fn from_json_line(line: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(line.trim()).ok()?;
        let obj = v.as_object()?;
        Some(Self {
            tool_name: obj.get("tool_name")?.as_str()?.to_owned(),
            args: obj
                .get("args")
                .and_then(|a| a.as_array().cloned())
                .unwrap_or_default(),
            kwargs: obj
                .get("kwargs")
                .and_then(|k| k.as_object().cloned())
                .unwrap_or_default(),
            ts: obj.get("ts").and_then(|t| t.as_f64()).unwrap_or(0.0),
            returned: obj.get("returned").cloned().unwrap_or(Value::Null),
        })
    }
}

// ---- ShadowMode -----------------------------------------------------------

/// Toggleable shadow-mode manager for agent tool calls.
pub struct ShadowMode {
    active: bool,
    records: Vec<ShadowRecord>,
    clock: Box<dyn Fn() -> f64 + Send>,
}

impl ShadowMode {
    /// Create a new instance. Pass `true` to start in shadow mode.
    pub fn new(active: bool) -> Self {
        Self {
            active,
            records: Vec::new(),
            clock: Box::new(system_time_secs),
        }
    }

    /// Create with a custom clock (useful for tests).
    pub fn with_clock(active: bool, clock: impl Fn() -> f64 + Send + 'static) -> Self {
        Self {
            active,
            records: Vec::new(),
            clock: Box::new(clock),
        }
    }

    // ---- state -------------------------------------------------------

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }

    // ---- records -----------------------------------------------------

    pub fn records(&self) -> &[ShadowRecord] {
        &self.records
    }

    pub fn clear_records(&mut self) {
        self.records.clear();
    }

    // ---- core API ----------------------------------------------------

    /// Intercept a tool call.
    ///
    /// If active: record the call, return `when_shadow` without calling `f`.
    /// If inactive: call `f` and return its result.
    ///
    /// - `args`: positional args (usually `json!([arg1, arg2])`)
    /// - `kwargs`: keyword args (usually `json!({"key": val})`)
    /// - `when_shadow`: the placeholder to return in shadow mode
    /// - `f`: the real implementation (only called when inactive)
    pub fn intercept<F>(
        &mut self,
        tool_name: &str,
        args: Value,
        kwargs: Value,
        when_shadow: Value,
        f: F,
    ) -> Value
    where
        F: FnOnce() -> Value,
    {
        if !self.active {
            return f();
        }
        let shadow_val = when_shadow;
        let rec = ShadowRecord {
            tool_name: tool_name.to_owned(),
            args: match &args {
                Value::Array(v) => v.clone(),
                _ => vec![args],
            },
            kwargs: match kwargs {
                Value::Object(m) => m,
                _ => Map::new(),
            },
            ts: (self.clock)(),
            returned: shadow_val.clone(),
        };
        self.records.push(rec);
        shadow_val
    }

    // ---- persistence -------------------------------------------------

    /// Save all records to `path` as JSONL (overwrites). Returns count.
    pub fn to_jsonl(&self, path: impl AsRef<Path>) -> std::io::Result<usize> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut out = String::new();
        for rec in &self.records {
            out.push_str(&rec.to_json_line());
            out.push('\n');
        }
        std::fs::write(path, out)?;
        Ok(self.records.len())
    }

    /// Load records from a JSONL file.
    pub fn load_jsonl(path: impl AsRef<Path>) -> std::io::Result<Vec<ShadowRecord>> {
        let text = std::fs::read_to_string(path)?;
        let recs = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(ShadowRecord::from_json_line)
            .collect();
        Ok(recs)
    }
}

fn system_time_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ---- tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shadow_ts() -> ShadowMode {
        ShadowMode::with_clock(true, || 1_700_000_000.0)
    }

    #[test]
    fn active_returns_shadow_value() {
        let mut s = shadow_ts();
        let r = s.intercept("charge", json!([]), json!({}), json!("shadowed"), || json!("real"));
        assert_eq!(r, json!("shadowed"));
    }

    #[test]
    fn inactive_returns_real_value() {
        let mut s = shadow_ts();
        s.deactivate();
        let r = s.intercept("charge", json!([]), json!({}), json!("shadowed"), || json!("real"));
        assert_eq!(r, json!("real"));
    }

    #[test]
    fn records_captured() {
        let mut s = shadow_ts();
        s.intercept("t1", json!([1, 2]), json!({"k": "v"}), json!(null), || json!(null));
        assert_eq!(s.records().len(), 1);
        let rec = &s.records()[0];
        assert_eq!(rec.tool_name, "t1");
        assert_eq!(rec.args, vec![json!(1), json!(2)]);
        assert_eq!(rec.kwargs["k"], json!("v"));
        assert_eq!(rec.ts, 1_700_000_000.0);
        assert_eq!(rec.returned, json!(null));
    }

    #[test]
    fn inactive_does_not_record() {
        let mut s = shadow_ts();
        s.deactivate();
        s.intercept("t1", json!([]), json!({}), json!(null), || json!(null));
        assert!(s.records().is_empty());
    }

    #[test]
    fn toggle_activate_deactivate() {
        let mut s = ShadowMode::new(false);
        assert!(!s.is_active());
        s.activate();
        assert!(s.is_active());
        s.deactivate();
        assert!(!s.is_active());
    }

    #[test]
    fn clear_records() {
        let mut s = shadow_ts();
        s.intercept("a", json!([]), json!({}), json!(1), || json!(1));
        s.intercept("b", json!([]), json!({}), json!(2), || json!(2));
        assert_eq!(s.records().len(), 2);
        s.clear_records();
        assert!(s.records().is_empty());
    }

    #[test]
    fn multiple_intercepts() {
        let mut s = shadow_ts();
        for i in 0..5u64 {
            s.intercept("t", json!([i]), json!({}), json!(i), || json!(i));
        }
        assert_eq!(s.records().len(), 5);
    }

    #[test]
    fn shadow_value_captured_in_record() {
        let mut s = shadow_ts();
        s.intercept("send_email", json!([]), json!({"to": "x@y.com"}), json!({"ok": true}), || unreachable!());
        assert_eq!(s.records()[0].returned, json!({"ok": true}));
    }

    #[test]
    fn kwargs_captured() {
        let mut s = shadow_ts();
        s.intercept("f", json!([]), json!({"a": 1, "b": "c"}), json!(null), || json!(null));
        let kw = &s.records()[0].kwargs;
        assert_eq!(kw["a"], json!(1));
        assert_eq!(kw["b"], json!("c"));
    }

    #[test]
    fn jsonl_round_trip() {
        let mut s = shadow_ts();
        s.intercept("tool_a", json!([42]), json!({"x": "y"}), json!({"status": "ok"}), || unreachable!());
        s.intercept("tool_b", json!([]), json!({}), json!(null), || unreachable!());

        let path = std::env::temp_dir().join("shadow_test_roundtrip.jsonl");
        let n = s.to_jsonl(&path).unwrap();
        assert_eq!(n, 2);

        let loaded = ShadowMode::load_jsonl(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].tool_name, "tool_a");
        assert_eq!(loaded[0].args[0], json!(42));
        assert_eq!(loaded[1].tool_name, "tool_b");
        assert_eq!(loaded[1].returned, Value::Null);

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn to_json_line_roundtrip() {
        let rec = ShadowRecord {
            tool_name: "foo".to_owned(),
            args: vec![json!(1), json!(2)],
            kwargs: serde_json::from_str(r#"{"k": "v"}"#).unwrap(),
            ts: 12345.6,
            returned: json!("ok"),
        };
        let line = rec.to_json_line();
        let back = ShadowRecord::from_json_line(&line).unwrap();
        assert_eq!(back.tool_name, "foo");
        assert_eq!(back.args, vec![json!(1), json!(2)]);
        assert_eq!(back.ts, 12345.6);
    }

    #[test]
    fn from_json_line_blank_returns_none() {
        assert!(ShadowRecord::from_json_line("").is_none());
        assert!(ShadowRecord::from_json_line("   ").is_none());
        assert!(ShadowRecord::from_json_line("not json").is_none());
    }

    #[test]
    fn non_array_args_wrapped() {
        let mut s = shadow_ts();
        // When args is not an array, wrap it in a vec
        s.intercept("t", json!("single_arg"), json!({}), json!(null), || json!(null));
        assert_eq!(s.records()[0].args, vec![json!("single_arg")]);
    }
}
