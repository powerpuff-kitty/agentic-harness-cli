//! Reject duplicate object keys, including escaped aliases, before policy use.
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) fn decode(bytes: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "checks: invalid JSON")?;
    // Syntax is already validated. A string followed by ':' can only be an object key.
    let mut objects: Vec<Option<BTreeSet<String>>> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => objects.push(Some(BTreeSet::new())),
            b'[' => objects.push(None),
            b'}' | b']' => {
                objects.pop();
            }
            b'"' => {
                let start = index;
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
                let end = index + 1;
                let mut next = end;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if bytes.get(next) == Some(&b':') {
                    let key: String = serde_json::from_slice(&bytes[start..end])
                        .map_err(|_| "checks: invalid JSON key")?;
                    let keys = objects
                        .last_mut()
                        .and_then(Option::as_mut)
                        .ok_or("checks: invalid JSON object")?;
                    if !keys.insert(key) {
                        return Err("checks: duplicate JSON object key (value omitted)".into());
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn rejects_duplicates_in_nested_objects_and_escaped_aliases() {
        for bytes in [
            br#"{"a":1,"a":2}"#.as_slice(),
            br#"{"checks":[{"cwd":".","cwd":"elsewhere"}]}"#,
            br#"{"a":1,"\u0061":2}"#,
            br#"{"environment":{"PATH":"","PATH":"/bin"}}"#,
        ] {
            assert!(decode(bytes).is_err());
        }
    }

    #[test]
    fn accepts_distinct_scopes_strings_and_escaped_quotes() {
        for bytes in [
            br#"{"a":{"key":1},"b":{"key":2}}"#.as_slice(),
            br#"[{"x":1},{"x":2}]"#,
            br#"{"text":"{\"key\":1}","key":2}"#,
            br#"{"a\\":1,"a\"":2}"#,
        ] {
            assert!(decode(bytes).is_ok());
        }
    }

    #[test]
    fn malformed_json_never_echoes_input() {
        let error = decode(b"{private-example").unwrap_err();
        assert!(!error.contains("private-example"));
    }
}
