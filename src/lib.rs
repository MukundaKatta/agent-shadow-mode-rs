/*!
agent-shadow-mode: record-not-execute wrapper for AI agent tools.

In shadow mode, tool calls are logged but the underlying function is NOT
called. Toggle shadow on/off at runtime; every call is appended to an
in-memory audit log.

```rust
use agent_shadow_mode::ShadowAgent;
use serde_json::json;

let mut agent = ShadowAgent::new(true); // shadow=true
let result = agent.execute("search", &json!({"q": "hello"}), || json!("result")).unwrap();
assert!(result.is_none()); // shadow: not executed
assert_eq!(agent.entries().len(), 1);
```
*/

use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// One recorded invocation.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub name: String,
    pub args: Value,
    /// Present when shadow=false and function succeeded.
    pub result: Option<Value>,
    /// Whether the call was shadowed (not executed).
    pub was_shadow: bool,
    pub timestamp: f64,
}

impl AuditEntry {
    /// Convert this entry into a JSON object suitable for logging or export.
    ///
    /// The `result` field is `null` when the call was shadowed (or the
    /// underlying function returned no value).
    pub fn to_value(&self) -> Value {
        json!({
            "name": self.name,
            "args": self.args,
            "result": self.result.clone().unwrap_or(Value::Null),
            "was_shadow": self.was_shadow,
            "timestamp": self.timestamp,
        })
    }
}

/// An agent wrapper that records tool calls and optionally suppresses execution.
#[derive(Clone)]
pub struct ShadowAgent {
    shadow: Arc<Mutex<bool>>,
    log: Arc<Mutex<Vec<AuditEntry>>>,
}

impl ShadowAgent {
    /// Create a new ShadowAgent. Pass `shadow=true` to suppress execution.
    pub fn new(shadow: bool) -> Self {
        Self {
            shadow: Arc::new(Mutex::new(shadow)),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Enable or disable shadow mode at runtime.
    pub fn set_shadow(&self, enabled: bool) {
        *self.shadow.lock().unwrap() = enabled;
    }

    /// True when currently in shadow mode.
    pub fn is_shadow(&self) -> bool {
        *self.shadow.lock().unwrap()
    }

    /// Execute (or shadow) a tool call.
    ///
    /// If shadow mode is active, `f` is NOT called and `Ok(None)` is returned.
    /// Otherwise `f` is called and its result is returned as `Ok(Some(value))`.
    pub fn execute<F>(&self, name: &str, args: &Value, f: F) -> Result<Option<Value>, String>
    where
        F: FnOnce() -> Value,
    {
        let is_shadow = self.is_shadow();
        let result = if is_shadow { None } else { Some(f()) };

        self.log.lock().unwrap().push(AuditEntry {
            name: name.to_owned(),
            args: args.clone(),
            result: result.clone(),
            was_shadow: is_shadow,
            timestamp: now_f64(),
        });

        Ok(result)
    }

    /// All audit entries in insertion order.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.log.lock().unwrap().clone()
    }

    /// Clear the audit log.
    pub fn clear(&self) {
        self.log.lock().unwrap().clear();
    }

    /// Count entries matching `name`.
    pub fn count(&self, name: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.name == name)
            .count()
    }

    /// True if any shadowed calls exist in the log.
    pub fn has_shadowed(&self) -> bool {
        self.log.lock().unwrap().iter().any(|e| e.was_shadow)
    }

    /// Serialize the audit log as JSON Lines (one JSON object per line).
    ///
    /// Each line is a compact JSON object with the fields `name`, `args`,
    /// `result`, `was_shadow` and `timestamp`. An empty log yields an empty
    /// string. The returned string does not have a trailing newline.
    ///
    /// ```
    /// use agent_shadow_mode::ShadowAgent;
    /// use serde_json::json;
    ///
    /// let agent = ShadowAgent::new(true);
    /// agent.execute("search", &json!({"q": "x"}), || json!(null)).unwrap();
    /// let jsonl = agent.to_jsonl();
    /// assert!(jsonl.contains("\"name\":\"search\""));
    /// assert!(jsonl.contains("\"was_shadow\":true"));
    /// ```
    pub fn to_jsonl(&self) -> String {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.to_value().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Write the audit log to `path` as JSON Lines.
    ///
    /// The file is created (or truncated) and a trailing newline is written
    /// after the last entry when the log is non-empty, so the file is valid
    /// JSONL that can be appended to or read line by line.
    pub fn write_jsonl<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        let mut data = self.to_jsonl();
        if !data.is_empty() {
            data.push('\n');
        }
        std::fs::write(path, data)
    }
}

impl std::fmt::Debug for ShadowAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShadowAgent")
            .field("shadow", &self.is_shadow())
            .field("entries", &self.entries().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shadow_mode_does_not_call_fn() {
        let agent = ShadowAgent::new(true);
        let mut called = false;
        let r = agent
            .execute("tool", &json!({}), || {
                called = true;
                json!("result")
            })
            .unwrap();
        assert!(!called);
        assert!(r.is_none());
    }

    #[test]
    fn non_shadow_calls_fn() {
        let agent = ShadowAgent::new(false);
        let r = agent
            .execute("tool", &json!({}), || json!("output"))
            .unwrap();
        assert_eq!(r, Some(json!("output")));
    }

    #[test]
    fn shadow_entry_logged() {
        let agent = ShadowAgent::new(true);
        agent
            .execute("search", &json!({"q": "x"}), || json!(null))
            .unwrap();
        let entries = agent.entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].was_shadow);
        assert_eq!(entries[0].name, "search");
        assert_eq!(entries[0].args, json!({"q": "x"}));
        assert!(entries[0].result.is_none());
    }

    #[test]
    fn non_shadow_entry_logged_with_result() {
        let agent = ShadowAgent::new(false);
        agent
            .execute("get", &json!({"id": 1}), || json!("data"))
            .unwrap();
        let entries = agent.entries();
        assert!(!entries[0].was_shadow);
        assert_eq!(entries[0].result, Some(json!("data")));
    }

    #[test]
    fn toggle_shadow_mid_run() {
        let agent = ShadowAgent::new(true);
        agent.execute("a", &json!({}), || json!(1)).unwrap();
        agent.set_shadow(false);
        agent.execute("b", &json!({}), || json!(2)).unwrap();
        let entries = agent.entries();
        assert!(entries[0].was_shadow);
        assert!(!entries[1].was_shadow);
        assert_eq!(entries[1].result, Some(json!(2)));
    }

    #[test]
    fn clear_resets_log() {
        let agent = ShadowAgent::new(false);
        agent.execute("x", &json!({}), || json!(null)).unwrap();
        agent.clear();
        assert!(agent.entries().is_empty());
    }

    #[test]
    fn count_by_name() {
        let agent = ShadowAgent::new(false);
        agent.execute("a", &json!({}), || json!(null)).unwrap();
        agent.execute("b", &json!({}), || json!(null)).unwrap();
        agent.execute("a", &json!({}), || json!(null)).unwrap();
        assert_eq!(agent.count("a"), 2);
        assert_eq!(agent.count("b"), 1);
        assert_eq!(agent.count("c"), 0);
    }

    #[test]
    fn has_shadowed_true() {
        let agent = ShadowAgent::new(true);
        agent.execute("t", &json!({}), || json!(null)).unwrap();
        assert!(agent.has_shadowed());
    }

    #[test]
    fn has_shadowed_false_when_none() {
        let agent = ShadowAgent::new(false);
        agent.execute("t", &json!({}), || json!(null)).unwrap();
        assert!(!agent.has_shadowed());
    }

    #[test]
    fn is_shadow_reflects_state() {
        let agent = ShadowAgent::new(false);
        assert!(!agent.is_shadow());
        agent.set_shadow(true);
        assert!(agent.is_shadow());
    }

    #[test]
    fn multiple_entries_in_order() {
        let agent = ShadowAgent::new(false);
        for i in 0..5u32 {
            agent.execute("t", &json!(i), || json!(i * 2)).unwrap();
        }
        let entries = agent.entries();
        assert_eq!(entries.len(), 5);
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(e.args, json!(i as u32));
        }
    }

    #[test]
    fn clone_shares_state() {
        let agent = ShadowAgent::new(false);
        let agent2 = agent.clone();
        agent.execute("t", &json!({}), || json!(null)).unwrap();
        assert_eq!(agent2.entries().len(), 1);
    }

    #[test]
    fn timestamp_is_set() {
        let agent = ShadowAgent::new(false);
        agent.execute("t", &json!({}), || json!(null)).unwrap();
        assert!(agent.entries()[0].timestamp > 0.0);
    }

    #[test]
    fn empty_log_has_no_shadowed() {
        let agent = ShadowAgent::new(true);
        assert!(!agent.has_shadowed());
    }

    #[test]
    fn to_jsonl_empty_is_empty_string() {
        let agent = ShadowAgent::new(true);
        assert_eq!(agent.to_jsonl(), "");
    }

    #[test]
    fn to_jsonl_one_line_per_entry() {
        let agent = ShadowAgent::new(false);
        agent.execute("a", &json!({}), || json!(1)).unwrap();
        agent.execute("b", &json!({}), || json!(2)).unwrap();
        let jsonl = agent.to_jsonl();
        assert_eq!(jsonl.lines().count(), 2);
    }

    #[test]
    fn to_jsonl_lines_are_valid_json() {
        let agent = ShadowAgent::new(true);
        agent
            .execute("search", &json!({"q": "x"}), || json!(null))
            .unwrap();
        let line = agent.to_jsonl();
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["name"], json!("search"));
        assert_eq!(parsed["args"], json!({"q": "x"}));
        assert_eq!(parsed["was_shadow"], json!(true));
        assert_eq!(parsed["result"], Value::Null);
    }

    #[test]
    fn to_value_uses_null_for_missing_result() {
        let entry = AuditEntry {
            name: "t".into(),
            args: json!({}),
            result: None,
            was_shadow: true,
            timestamp: 1.0,
        };
        assert_eq!(entry.to_value()["result"], Value::Null);
    }

    #[test]
    fn write_jsonl_round_trips() {
        let agent = ShadowAgent::new(false);
        agent.execute("a", &json!({"k": 1}), || json!("r")).unwrap();
        agent.execute("b", &json!({"k": 2}), || json!("s")).unwrap();

        let mut path = std::env::temp_dir();
        path.push(format!("shadow-test-{}.jsonl", now_f64() as u128));
        agent.write_jsonl(&path).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(contents.ends_with('\n'));
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["name"], json!("a"));
        assert_eq!(first["result"], json!("r"));
    }

    #[test]
    fn write_jsonl_empty_log_writes_empty_file() {
        let agent = ShadowAgent::new(true);
        let mut path = std::env::temp_dir();
        path.push(format!("shadow-empty-{}.jsonl", now_f64() as u128));
        agent.write_jsonl(&path).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(contents, "");
    }
}
