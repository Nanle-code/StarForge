use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DiffChange {
    Added,
    Removed,
    Modified,
    Unchanged,
    TypeChanged,
    Renamed,
}

impl std::fmt::Display for DiffChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffChange::Added => write!(f, "added"),
            DiffChange::Removed => write!(f, "removed"),
            DiffChange::Modified => write!(f, "modified"),
            DiffChange::Unchanged => write!(f, "unchanged"),
            DiffChange::TypeChanged => write!(f, "type-changed"),
            DiffChange::Renamed => write!(f, "renamed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    pub key: String,
    pub change: DiffChange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiffReport {
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub total_keys_before: usize,
    pub total_keys_after: usize,
    pub added: Vec<DiffEntry>,
    pub removed: Vec<DiffEntry>,
    pub modified: Vec<DiffEntry>,
    pub unchanged: Vec<DiffEntry>,
    pub type_changed: Vec<DiffEntry>,
    pub renamed: Vec<DiffEntry>,
    pub summary: DiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummary {
    pub count_added: usize,
    pub count_removed: usize,
    pub count_modified: usize,
    pub count_unchanged: usize,
    pub count_type_changed: usize,
    pub count_renamed: usize,
    pub total_changes: usize,
}

fn json_type_name(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Object(_) => "object".to_string(),
    }
}

pub fn diff_snapshots(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    from_version: Option<String>,
    to_version: Option<String>,
) -> StateDiffReport {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();
    let mut unchanged = Vec::new();
    let mut type_changed = Vec::new();
    let mut renamed = Vec::new();

    let mut candidate_removed: BTreeMap<String, Value> = BTreeMap::new();

    for (key, before_val) in before.iter() {
        match after.get(key) {
            None => {
                candidate_removed.insert(key.clone(), before_val.clone());
            }
            Some(after_val) => {
                let before_type = json_type_name(before_val);
                let after_type = json_type_name(after_val);

                if before_type != after_type {
                    type_changed.push(DiffEntry {
                        key: key.clone(),
                        change: DiffChange::TypeChanged,
                        before: Some(before_val.clone()),
                        after: Some(after_val.clone()),
                        before_type: Some(before_type),
                        after_type: Some(after_type),
                        renamed_from: None,
                    });
                } else if before_val == after_val {
                    unchanged.push(DiffEntry {
                        key: key.clone(),
                        change: DiffChange::Unchanged,
                        before: Some(before_val.clone()),
                        after: Some(after_val.clone()),
                        before_type: Some(before_type),
                        after_type: Some(after_type),
                        renamed_from: None,
                    });
                } else {
                    modified.push(DiffEntry {
                        key: key.clone(),
                        change: DiffChange::Modified,
                        before: Some(before_val.clone()),
                        after: Some(after_val.clone()),
                        before_type: Some(before_type),
                        after_type: Some(after_type),
                        renamed_from: None,
                    });
                }
            }
        }
    }

    let mut matched_removals = std::collections::BTreeSet::new();

    for (key, after_val) in after.iter() {
        if !before.contains_key(key) {
            // Check if this added key matches a removed key with the same value (rename detection)
            let rename_match = candidate_removed
                .iter()
                .find(|(r_key, r_val)| !matched_removals.contains(*r_key) && *r_val == after_val);

            if let Some((r_key, r_val)) = rename_match {
                matched_removals.insert(r_key.clone());
                renamed.push(DiffEntry {
                    key: key.clone(),
                    change: DiffChange::Renamed,
                    before: Some(r_val.clone()),
                    after: Some(after_val.clone()),
                    before_type: Some(json_type_name(r_val)),
                    after_type: Some(json_type_name(after_val)),
                    renamed_from: Some(r_key.clone()),
                });
            } else {
                let after_type = json_type_name(after_val);
                added.push(DiffEntry {
                    key: key.clone(),
                    change: DiffChange::Added,
                    before: None,
                    after: Some(after_val.clone()),
                    before_type: None,
                    after_type: Some(after_type),
                    renamed_from: None,
                });
            }
        }
    }

    for (r_key, r_val) in candidate_removed {
        if !matched_removals.contains(&r_key) {
            removed.push(DiffEntry {
                key: r_key.clone(),
                change: DiffChange::Removed,
                before: Some(r_val.clone()),
                after: None,
                before_type: Some(json_type_name(&r_val)),
                after_type: None,
                renamed_from: None,
            });
        }
    }

    let summary = DiffSummary {
        count_added: added.len(),
        count_removed: removed.len(),
        count_modified: modified.len(),
        count_unchanged: unchanged.len(),
        count_type_changed: type_changed.len(),
        count_renamed: renamed.len(),
        total_changes: added.len()
            + removed.len()
            + modified.len()
            + type_changed.len()
            + renamed.len(),
    };

    StateDiffReport {
        from_version,
        to_version,
        total_keys_before: before.len(),
        total_keys_after: after.len(),
        added,
        removed,
        modified,
        unchanged,
        type_changed,
        renamed,
        summary,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffGeneratedRule {
    pub key: String,
    pub op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_type: Option<String>,
}

pub fn generate_migration_rules_from_diff(report: &StateDiffReport) -> Vec<DiffGeneratedRule> {
    let mut rules = Vec::new();

    for entry in &report.renamed {
        if let Some(ref from) = entry.renamed_from {
            rules.push(DiffGeneratedRule {
                key: entry.key.clone(),
                op: "rename_key".to_string(),
                from_key: Some(from.clone()),
                default_value: None,
                target_type: None,
            });
        }
    }

    for entry in &report.removed {
        rules.push(DiffGeneratedRule {
            key: entry.key.clone(),
            op: "remove_field".to_string(),
            from_key: None,
            default_value: None,
            target_type: None,
        });
    }

    for entry in &report.added {
        rules.push(DiffGeneratedRule {
            key: entry.key.clone(),
            op: "add_field".to_string(),
            from_key: None,
            default_value: entry.after.clone(),
            target_type: None,
        });
    }

    for entry in &report.type_changed {
        rules.push(DiffGeneratedRule {
            key: entry.key.clone(),
            op: "cast_type".to_string(),
            from_key: None,
            default_value: None,
            target_type: entry.after_type.clone(),
        });
    }

    rules
}

pub fn render_diff_console(report: &StateDiffReport) -> String {
    use colored::Colorize;
    let mut out = String::new();

    out.push_str("State Diff Report\n");
    out.push_str(&"=".repeat(64));
    out.push('\n');

    if let Some(v) = &report.from_version {
        out.push_str(&format!("  From version: {}\n", v.bright_white()));
    }
    if let Some(v) = &report.to_version {
        out.push_str(&format!("  To version:   {}\n", v.bright_white()));
    }
    out.push_str(&format!(
        "  Total keys before: {}\n",
        report.total_keys_before
    ));
    out.push_str(&format!(
        "  Total keys after:  {}\n",
        report.total_keys_after
    ));
    out.push_str(&format!(
        "  Changes: +{}/ -{} ~{} ⟳{} ->{} (unchanged: {})\n",
        report.summary.count_added,
        report.summary.count_removed,
        report.summary.count_modified,
        report.summary.count_type_changed,
        report.summary.count_renamed,
        report.summary.count_unchanged,
    ));
    out.push_str(&"=".repeat(64));
    out.push('\n');

    if !report.renamed.is_empty() {
        out.push_str(&format!(
            "\n  {} Renamed ({})\n",
            "->".magenta().bold(),
            report.renamed.len()
        ));
        out.push_str(&format!("  {}\n", "-".repeat(40)));
        for entry in &report.renamed {
            let from_k = entry.renamed_from.as_deref().unwrap_or("?");
            out.push_str(&format!("  -> {} -> {}\n", from_k.red(), entry.key.green()));
        }
    }

    if !report.added.is_empty() {
        out.push_str(&format!(
            "\n  {} Added ({})\n",
            "+".green().bold(),
            report.added.len()
        ));
        out.push_str(&format!("  {}\n", "-".repeat(40)));
        for entry in &report.added {
            let val = entry
                .after
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".to_string());
            out.push_str(&format!("  + {} = {}\n", entry.key.green(), val.dimmed()));
        }
    }

    if !report.removed.is_empty() {
        out.push_str(&format!(
            "\n  {} Removed ({})\n",
            "-".red().bold(),
            report.removed.len()
        ));
        out.push_str(&format!("  {}\n", "-".repeat(40)));
        for entry in &report.removed {
            let val = entry
                .before
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".to_string());
            out.push_str(&format!("  - {} = {}\n", entry.key.red(), val.dimmed()));
        }
    }

    if !report.modified.is_empty() {
        out.push_str(&format!(
            "\n  {} Modified ({})\n",
            "~".yellow().bold(),
            report.modified.len()
        ));
        out.push_str(&format!("  {}\n", "-".repeat(40)));
        for entry in &report.modified {
            let before_val = entry
                .before
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".to_string());
            let after_val = entry
                .after
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".to_string());
            out.push_str(&format!(
                "  ~ {}: {} → {}\n",
                entry.key.yellow(),
                before_val.red(),
                after_val.green()
            ));
        }
    }

    if !report.type_changed.is_empty() {
        out.push_str(&format!(
            "\n  {} Type Changed ({})\n",
            "⟳".cyan().bold(),
            report.type_changed.len()
        ));
        out.push_str(&format!("  {}\n", "-".repeat(40)));
        for entry in &report.type_changed {
            let bt = entry.before_type.as_deref().unwrap_or("?");
            let at = entry.after_type.as_deref().unwrap_or("?");
            out.push_str(&format!(
                "  ⟳ {}: {} → {}\n",
                entry.key.cyan(),
                bt.yellow(),
                at.green()
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map_from(v: Vec<(&str, Value)>) -> BTreeMap<String, Value> {
        v.into_iter().map(|(k, val)| (k.to_string(), val)).collect()
    }

    #[test]
    fn detects_added_keys() {
        let before = map_from(vec![("a", json!(1))]);
        let after = map_from(vec![("a", json!(1)), ("b", json!(2))]);
        let report = diff_snapshots(&before, &after, None, None);
        assert_eq!(report.summary.count_added, 1);
        assert_eq!(report.summary.count_unchanged, 1);
        assert_eq!(report.added[0].key, "b");
    }

    #[test]
    fn detects_removed_keys() {
        let before = map_from(vec![("a", json!(1)), ("b", json!(2))]);
        let after = map_from(vec![("a", json!(1))]);
        let report = diff_snapshots(&before, &after, None, None);
        assert_eq!(report.summary.count_removed, 1);
        assert_eq!(report.summary.count_unchanged, 1);
        assert_eq!(report.removed[0].key, "b");
    }

    #[test]
    fn detects_renamed_keys() {
        let before = map_from(vec![("old_name", json!("val_123"))]);
        let after = map_from(vec![("new_name", json!("val_123"))]);
        let report = diff_snapshots(&before, &after, None, None);
        assert_eq!(report.summary.count_renamed, 1);
        assert_eq!(report.renamed[0].key, "new_name");
        assert_eq!(report.renamed[0].renamed_from.as_deref(), Some("old_name"));

        let rules = generate_migration_rules_from_diff(&report);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].op, "rename_key");
        assert_eq!(rules[0].from_key.as_deref(), Some("old_name"));
        assert_eq!(rules[0].key, "new_name");
    }

    #[test]
    fn detects_modified_values() {
        let before = map_from(vec![("a", json!(1))]);
        let after = map_from(vec![("a", json!(2))]);
        let report = diff_snapshots(&before, &after, None, None);
        assert_eq!(report.summary.count_modified, 1);
        assert_eq!(report.modified[0].key, "a");
    }

    #[test]
    fn detects_type_changes() {
        let before = map_from(vec![("a", json!(1))]);
        let after = map_from(vec![("a", json!("1"))]);
        let report = diff_snapshots(&before, &after, None, None);
        assert_eq!(report.summary.count_type_changed, 1);
        assert_eq!(report.type_changed[0].key, "a");
        assert_eq!(
            report.type_changed[0].before_type,
            Some("number".to_string())
        );
        assert_eq!(
            report.type_changed[0].after_type,
            Some("string".to_string())
        );
    }

    #[test]
    fn empty_snapshots_produce_no_diff() {
        let before: BTreeMap<String, Value> = BTreeMap::new();
        let after: BTreeMap<String, Value> = BTreeMap::new();
        let report = diff_snapshots(&before, &after, None, None);
        assert_eq!(report.summary.total_changes, 0);
    }

    #[test]
    fn renders_console_output() {
        let before = map_from(vec![("balance", json!("100")), ("owner", json!("GA..."))]);
        let after = map_from(vec![
            ("balance", json!("200")),
            ("owner", json!("GA...")),
            ("admin", json!("GB...")),
        ]);
        let report = diff_snapshots(&before, &after, Some("v1".into()), Some("v2".into()));
        let rendered = render_diff_console(&report);
        assert!(rendered.contains("v1"));
        assert!(rendered.contains("v2"));
        assert!(rendered.contains("admin"));
        assert!(rendered.contains("balance"));
    }

    #[test]
    fn generates_migration_rules() {
        let before = map_from(vec![("old_field", json!("hi"))]);
        let after = map_from(vec![("new_field", json!("hello")), ("schema", json!("v2"))]);
        let report = diff_snapshots(&before, &after, None, None);
        let rules = generate_migration_rules_from_diff(&report);

        assert!(rules
            .iter()
            .any(|r| r.op == "remove_field" && r.key == "old_field"));
        assert!(rules
            .iter()
            .any(|r| r.op == "add_field" && r.key == "new_field"));
        assert!(rules
            .iter()
            .any(|r| r.op == "add_field" && r.key == "schema"));
    }
}
