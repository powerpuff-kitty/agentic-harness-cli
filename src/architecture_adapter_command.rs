use crate::architecture_adapters;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn normalize_nx(mut result: Value) -> Result<Value, String> {
    let content = result["content"]
        .as_str()
        .ok_or_else(|| "Nx adapter did not produce textual content".to_string())?;
    let mut artifact: Value = serde_json::from_str(content)
        .map_err(|error| format!("invalid generated Nx adapter JSON: {error}"))?;

    let constraints = artifact["depConstraints"]
        .as_array_mut()
        .ok_or_else(|| "generated Nx adapter is missing depConstraints".to_string())?;
    let mut rule_map = Vec::new();
    for (index, constraint) in constraints.iter_mut().enumerate() {
        if let Some(object) = constraint.as_object_mut()
            && let Some(rule_id) = object.remove("source_rule_id")
        {
            rule_map.push(json!({
                "constraint_index": index,
                "source_rule_id": rule_id
            }));
        }
    }
    artifact["source_rule_map"] = json!(rule_map);

    let mut normalized = serde_json::to_string_pretty(&artifact)
        .map_err(|error| format!("failed to serialize normalized Nx adapter: {error}"))?;
    normalized.push('\n');
    result["content"] = json!(normalized);
    result["native_fragment_validated"] = json!(true);
    Ok(result)
}

fn normalize(adapter: &str, result: Value) -> Result<Value, String> {
    match adapter {
        "nx" => normalize_nx(result),
        _ => Ok(result),
    }
}

pub fn generate(root: &Path, adapter: &str, write: bool) -> Result<Value, String> {
    let preview = architecture_adapters::generate(root, adapter, false)?;
    let mut result = normalize(adapter, preview)?;
    result["write"] = json!(write);

    if write {
        if !result["applicable"].as_bool().unwrap_or(false) {
            return Err(format!(
                "adapter `{adapter}` is not applicable to this project; refusing to write {}",
                result["path"].as_str().unwrap_or("generated adapter")
            ));
        }
        let relative = result["path"]
            .as_str()
            .ok_or_else(|| "generated adapter is missing its output path".to_string())?;
        let destination = root.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        let content = result["content"]
            .as_str()
            .ok_or_else(|| "generated adapter is missing textual content".to_string())?;
        fs::write(&destination, content)
            .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture_contract;
    use std::env;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "ah-architecture-adapter-command-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn nx_native_constraints_do_not_contain_harness_metadata() {
        let root = fixture("nx-native");
        fs::write(root.join("nx.json"), "{}").unwrap();
        architecture_contract::write(&root, &["pattern/layered/1".to_string()]).unwrap();
        let result = generate(&root, "nx", false).unwrap();
        let artifact: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert!(artifact["depConstraints"]
            .as_array()
            .unwrap()
            .iter()
            .all(|constraint| constraint.get("source_rule_id").is_none()));
        assert!(!artifact["source_rule_map"].as_array().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preview_never_writes_and_write_uses_normalized_content() {
        let root = fixture("write-normalized");
        fs::write(root.join("nx.json"), "{}").unwrap();
        architecture_contract::write(&root, &["pattern/layered/1".to_string()]).unwrap();
        let preview = generate(&root, "nx", false).unwrap();
        let path = root.join(preview["path"].as_str().unwrap());
        assert!(!path.exists());
        let written = generate(&root, "nx", true).unwrap();
        assert!(path.exists());
        assert_eq!(fs::read_to_string(path).unwrap(), written["content"].as_str().unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
