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

use serde_json::Value;
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
        self.log.lock().unwrap().iter().filter(|e| e.name == name).count()
    }

    /// True if any shadowed calls exist in the log.
    pub fn has_shadowed(&self) -> bool {
        self.log.lock().unwrap().iter().any(|e| e.was_shadow)
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
        let r = agent.execute("tool", &json!({}), || {
            called = true;
            json!("result")
        }).unwrap();
        assert!(!called);
        assert!(r.is_none());
    }

    #[test]
    fn non_shadow_calls_fn() {
        let agent = ShadowAgent::new(false);
        let r = agent.execute("tool", &json!({}), || json!("output")).unwrap();
        assert_eq!(r, Some(json!("output")));
    }

    #[test]
    fn shadow_entry_logged() {
        let agent = ShadowAgent::new(true);
        agent.execute("search", &json!({"q": "x"}), || json!(null)).unwrap();
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
        agent.execute("get", &json!({"id": 1}), || json!("data")).unwrap();
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
}
