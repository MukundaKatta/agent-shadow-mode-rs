//! Integration tests exercising the public `agent_shadow_mode` API.

use agent_shadow_mode::ShadowAgent;
use serde_json::json;

#[test]
fn shadow_mode_suppresses_side_effects() {
    let agent = ShadowAgent::new(true);
    let mut side_effect_ran = false;

    let result = agent
        .execute("charge_card", &json!({"amount": 499}), || {
            side_effect_ran = true;
            json!({"status": "charged"})
        })
        .unwrap();

    assert!(
        !side_effect_ran,
        "real function must not run in shadow mode"
    );
    assert!(result.is_none(), "shadow execution returns None");
    assert_eq!(agent.entries().len(), 1);
    assert!(agent.has_shadowed());
}

#[test]
fn live_mode_executes_and_records_result() {
    let agent = ShadowAgent::new(false);

    let result = agent
        .execute("lookup", &json!({"id": 7}), || json!({"name": "ada"}))
        .unwrap();

    assert_eq!(result, Some(json!({"name": "ada"})));
    let entries = agent.entries();
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].was_shadow);
    assert_eq!(entries[0].result, Some(json!({"name": "ada"})));
}

#[test]
fn toggling_shadow_at_runtime_changes_behavior() {
    let agent = ShadowAgent::new(true);

    // First call: shadowed.
    let r1 = agent.execute("step", &json!(1), || json!("a")).unwrap();
    assert!(r1.is_none());

    // Flip to live and run for real.
    agent.set_shadow(false);
    assert!(!agent.is_shadow());
    let r2 = agent.execute("step", &json!(2), || json!("b")).unwrap();
    assert_eq!(r2, Some(json!("b")));

    let entries = agent.entries();
    assert!(entries[0].was_shadow);
    assert!(!entries[1].was_shadow);
}

#[test]
fn audit_log_exports_as_valid_jsonl() {
    let agent = ShadowAgent::new(true);
    agent
        .execute("a", &json!({"x": 1}), || json!(null))
        .unwrap();
    agent.set_shadow(false);
    agent
        .execute("b", &json!({"x": 2}), || json!("done"))
        .unwrap();

    let jsonl = agent.to_jsonl();
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 2);

    // Every line must independently parse as a JSON object.
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("name").is_some());
        assert!(v.get("was_shadow").is_some());
        assert!(v.get("timestamp").is_some());
    }

    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["was_shadow"], json!(true));
    assert_eq!(first["result"], serde_json::Value::Null);

    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["was_shadow"], json!(false));
    assert_eq!(second["result"], json!("done"));
}

#[test]
fn clear_and_count_track_named_calls() {
    let agent = ShadowAgent::new(false);
    agent.execute("search", &json!({}), || json!(null)).unwrap();
    agent.execute("search", &json!({}), || json!(null)).unwrap();
    agent.execute("write", &json!({}), || json!(null)).unwrap();

    assert_eq!(agent.count("search"), 2);
    assert_eq!(agent.count("write"), 1);
    assert_eq!(agent.count("missing"), 0);

    agent.clear();
    assert!(agent.entries().is_empty());
    assert_eq!(agent.count("search"), 0);
}

#[test]
fn cloned_agents_share_log_and_mode() {
    let agent = ShadowAgent::new(true);
    let twin = agent.clone();

    agent.execute("t", &json!({}), || json!(null)).unwrap();
    assert_eq!(twin.entries().len(), 1, "clones share the audit log");

    twin.set_shadow(false);
    assert!(!agent.is_shadow(), "clones share the shadow flag");
}
