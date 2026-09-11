use std::path::Path;
use std::sync::LazyLock;
use tree_sitter::{Node, Parser};

static SCRIPTS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)<script\b[^>]*>(.*?)</script\s*>").unwrap());
pub fn script_source(path: &Path, text: &str) -> String {
    if path
        .extension()
        .is_some_and(|s| s == "vue" || s == "svelte")
    {
        let mut bytes = text
            .as_bytes()
            .iter()
            .map(|b| if *b == b'\n' { b'\n' } else { b' ' })
            .collect::<Vec<_>>();
        for capture in SCRIPTS.captures_iter(text) {
            let part = capture.get(1).unwrap();
            bytes[part.range()].copy_from_slice(part.as_str().as_bytes());
        }
        String::from_utf8(bytes).unwrap()
    } else {
        text.to_string()
    }
}
pub fn parse(path: &Path, text: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    let language = if path.extension().is_some_and(|s| s == "tsx" || s == "jsx") {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    parser
        .set_language(&language.into())
        .expect("bundled TypeScript grammar");
    parser
        .parse(text, None)
        .expect("parser has no cancellation")
}
pub fn walk<'a>(root: Node<'a>) -> Vec<Node<'a>> {
    let mut nodes = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        nodes.push(node);
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    nodes
}
pub fn imports(path: &Path, text: &str) -> (Vec<(usize, String, String)>, bool) {
    let source = script_source(path, text);
    let tree = parse(path, &source);
    let mut out = Vec::new();
    for node in walk(tree.root_node()) {
        let (literal, kind) = match node.kind() {
            "import_statement" | "export_statement" => {
                let statement = node.utf8_text(source.as_bytes()).unwrap_or("");
                let nodes = walk(node);
                let names: Vec<_> = nodes
                    .iter()
                    .filter(|n| ["import_specifier", "export_specifier"].contains(&n.kind()))
                    .collect();
                let clause = node.child_by_field_name("source");
                let only_types = statement.starts_with("import type ")
                    || statement.starts_with("export type ")
                    || (!names.is_empty()
                        && names.iter().all(|n| {
                            n.utf8_text(source.as_bytes())
                                .unwrap_or("")
                                .trim_start()
                                .starts_with("type ")
                        })
                        && !nodes.iter().any(|n| {
                            n.kind() == "namespace_import"
                                || (n.kind() == "import_clause"
                                    && n.named_child(0).is_some_and(|c| c.kind() == "identifier"))
                        }));
                (clause, if only_types { "type" } else { "runtime" })
            }
            "call_expression" => {
                let function = node
                    .child_by_field_name("function")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("");
                if !["import", "require"].contains(&function) {
                    continue;
                }
                (
                    node.child_by_field_name("arguments")
                        .and_then(|n| n.named_child(0)),
                    if function == "import" {
                        "dynamic"
                    } else {
                        "runtime"
                    },
                )
            }
            _ => continue,
        };
        if let Some(literal) = literal.filter(|n| n.kind() == "string") {
            let raw = literal.utf8_text(source.as_bytes()).unwrap_or("");
            if raw.len() >= 2 && !raw.contains('\\') {
                out.push((
                    node.start_position().row + 1,
                    raw[1..raw.len() - 1].to_string(),
                    kind.to_string(),
                ));
            }
        }
    }
    out.sort();
    out.dedup();
    (out, tree.root_node().has_error())
}
