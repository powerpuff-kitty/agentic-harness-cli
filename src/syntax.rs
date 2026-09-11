use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;
use std::path::Path;
use std::sync::LazyLock;

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
        static HTML_COMMENTS: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"(?s)<!--.*?-->").unwrap());
        let comment_ranges: Vec<_> = HTML_COMMENTS.find_iter(text).map(|m| m.range()).collect();
        for capture in SCRIPTS.captures_iter(text) {
            if comment_ranges
                .iter()
                .any(|r| r.contains(&capture.get(0).unwrap().start()))
            {
                continue;
            }
            let part = capture.get(1).unwrap();
            bytes[part.range()].copy_from_slice(part.as_str().as_bytes());
        }
        String::from_utf8(bytes).unwrap()
    } else {
        text.to_string()
    }
}
// Keep byte offsets aligned with the original SFC when reporting evidence lines.
pub(crate) struct SourceVisitor {
    newlines: Vec<usize>,
    unsupported: Vec<usize>,
    pub(crate) imports: Vec<(usize, String, String)>,
    pub(crate) tags: Vec<String>,
}
impl SourceVisitor {
    fn new(source: &str) -> Self {
        Self {
            newlines: source
                .match_indices('\n')
                .map(|(offset, _)| offset)
                .collect(),
            unsupported: Vec::new(),
            imports: Vec::new(),
            tags: Vec::new(),
        }
    }
    fn line(&self, offset: usize) -> usize {
        self.newlines.partition_point(|n| *n < offset) + 1
    }
    fn record(&mut self, offset: u32, name: &str, kind: &str) {
        self.imports
            .push((self.line(offset as usize), name.into(), kind.into()));
    }
}
fn edge_kind(type_only: bool) -> &'static str {
    if type_only { "type" } else { "runtime" }
}
impl<'a> Visit<'a> for SourceVisitor {
    fn visit_import_declaration(&mut self, node: &ImportDeclaration<'a>) {
        if node.phase.is_some() {
            // Source/deferred evaluation is not synchronous runtime evaluation.
            self.unsupported.push(self.line(node.span.start as usize));
            return;
        }
        let type_only = node.import_kind == ImportOrExportKind::Type
            || node.specifiers.as_ref().is_some_and(|specifiers| {
                !specifiers.is_empty()
                    && specifiers.iter().all(|s| {
                        matches!(s,
                    ImportDeclarationSpecifier::ImportSpecifier(s)
                        if s.import_kind == ImportOrExportKind::Type)
                    })
            });
        self.record(
            node.span.start,
            node.source.value.as_str(),
            edge_kind(type_only),
        );
        walk::walk_import_declaration(self, node);
    }
    fn visit_export_named_declaration(&mut self, node: &ExportNamedDeclaration<'a>) {
        if let Some(source) = &node.source {
            let type_only = node.export_kind == ImportOrExportKind::Type
                || (!node.specifiers.is_empty()
                    && node
                        .specifiers
                        .iter()
                        .all(|s| s.export_kind == ImportOrExportKind::Type));
            self.record(node.span.start, source.value.as_str(), edge_kind(type_only));
        }
        walk::walk_export_named_declaration(self, node);
    }
    fn visit_export_all_declaration(&mut self, node: &ExportAllDeclaration<'a>) {
        self.record(
            node.span.start,
            node.source.value.as_str(),
            edge_kind(node.export_kind == ImportOrExportKind::Type),
        );
        walk::walk_export_all_declaration(self, node);
    }
    fn visit_ts_import_type(&mut self, node: &TSImportType<'a>) {
        self.record(node.span.start, node.source.value.as_str(), "type");
        walk::walk_ts_import_type(self, node);
    }
    fn visit_ts_import_equals_declaration(&mut self, node: &TSImportEqualsDeclaration<'a>) {
        if let TSModuleReference::ExternalModuleReference(reference) = &node.module_reference {
            self.record(
                node.span.start,
                reference.expression.value.as_str(),
                edge_kind(node.import_kind == ImportOrExportKind::Type),
            );
        }
        walk::walk_ts_import_equals_declaration(self, node);
    }
    fn visit_import_expression(&mut self, node: &ImportExpression<'a>) {
        if node.phase.is_some() {
            self.unsupported.push(self.line(node.span.start as usize));
            return;
        }
        match &node.source {
            Expression::StringLiteral(s) => {
                self.record(node.span.start, s.value.as_str(), "dynamic")
            }
            Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
                if let Some(s) = t.quasis.first().and_then(|q| q.value.cooked.as_ref()) {
                    self.record(node.span.start, s.as_str(), "dynamic");
                }
            }
            _ => {} // Computed targets remain explicitly outside supported resolution.
        }
        walk::walk_import_expression(self, node);
    }
    fn visit_call_expression(&mut self, node: &CallExpression<'a>) {
        if matches!(&node.callee, Expression::Identifier(id) if id.name == "require")
            && let Some(Argument::StringLiteral(s)) = node.arguments.first()
        {
            self.record(node.span.start, s.value.as_str(), "runtime");
        }
        walk::walk_call_expression(self, node);
    }
    fn visit_jsx_opening_element(&mut self, node: &JSXOpeningElement<'a>) {
        match &node.name {
            JSXElementName::Identifier(id) => self.tags.push(id.name.to_string()),
            JSXElementName::IdentifierReference(id) => self.tags.push(id.name.to_string()),
            _ => {}
        }
        walk::walk_jsx_opening_element(self, node);
    }
}
pub(crate) fn analyze(path: &Path, source: &str) -> (SourceVisitor, Vec<usize>) {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::ts());
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut visitor = SourceVisitor::new(source);
    let mut gaps = Vec::new();
    for diagnostic in &parsed.diagnostics {
        if diagnostic.labels.is_empty() {
            gaps.push(1);
        } else {
            gaps.extend(
                diagnostic
                    .labels
                    .iter()
                    .map(|label| visitor.line(label.offset() as usize)),
            );
        }
    }
    if parsed.panicked && gaps.is_empty() {
        gaps.push(1);
    }
    visitor.visit_program(&parsed.program);
    gaps.append(&mut visitor.unsupported);
    gaps.sort_unstable();
    gaps.dedup();
    (visitor, gaps)
}
fn isolated(path: &Path, source: &str) -> crate::syntax_worker::Parsed {
    // Unit tests exercise the AST visitor directly. CLI integration tests exercise
    // the real worker, including process failure and recovery.
    #[cfg(test)]
    {
        let (visitor, gaps) = analyze(path, source);
        (visitor.imports, gaps, visitor.tags)
    }
    #[cfg(not(test))]
    crate::syntax_worker::analyze(path, source)
}
pub fn imports(path: &Path, text: &str) -> (Vec<(usize, String, String)>, Vec<usize>) {
    let source = script_source(path, text);
    let (mut imports, gaps, _) = isolated(path, &source);
    imports.sort();
    imports.dedup();
    (imports, gaps)
}
pub fn jsx_tags(path: &Path, text: &str) -> (Vec<String>, Vec<usize>) {
    let (_, gaps, tags) = isolated(path, text);
    (tags, gaps)
}

#[cfg(test)]
mod parser_tests {
    use super::*;
    #[test]
    fn imported_generic_type_edges_are_never_runtime() {
        let source = "import /* comment */ type { Env } from './env';\nexport type App = import('library').App<{ Bindings: Env; Variables: { auth: Env }; }>;";
        let (edges, gaps) = imports(Path::new("main.ts"), source);
        assert!(gaps.is_empty(), "{gaps:?}");
        assert!(
            edges
                .iter()
                .any(|(_, name, kind)| name == "./env" && kind == "type")
        );
        assert!(
            edges
                .iter()
                .any(|(_, name, kind)| name == "library" && kind == "type")
        );
        assert!(!edges.iter().any(|(_, _, kind)| kind == "runtime"));
    }
    #[test]
    fn compiler_valid_fixtures_preserve_dependency_kinds() {
        let source = include_str!("../tests/typescript/fixtures/imports.ts");
        let (edges, gaps) = imports(Path::new("imports.ts"), source);
        assert!(gaps.is_empty(), "{gaps:?}");
        let expected = [
            (1, "type"),
            (2, "type"),
            (3, "runtime"),
            (4, "runtime"),
            (5, "runtime"),
            (6, "runtime"),
            (7, "type"),
            (8, "type"),
            (9, "type"),
            (10, "type"),
            (11, "runtime"),
            (12, "type"),
            (16, "type"),
            (17, "type"),
            (18, "dynamic"),
            (19, "dynamic"),
            (20, "runtime"),
        ];
        assert_eq!(
            edges,
            expected.map(|(line, kind)| (line, "./env".into(), kind.into()))
        );
        let (edges, gaps) = imports(
            Path::new("common.cts"),
            include_str!("../tests/typescript/fixtures/common.cts"),
        );
        assert!(gaps.is_empty(), "{gaps:?}");
        assert_eq!(
            edges,
            [(1, "runtime"), (2, "type"), (4, "runtime")].map(|(line, kind)| (
                line,
                "./env".into(),
                kind.into()
            ))
        );
    }
    #[test]
    fn compiler_valid_tsx_uses_elements_not_strings() {
        let source = include_str!("../tests/typescript/fixtures/ui.tsx");
        let (edges, gaps) = imports(Path::new("ui.tsx"), source);
        assert!(gaps.is_empty(), "{gaps:?}");
        assert_eq!(edges, [(1, "./env".into(), "type".into())]);
        assert_eq!(jsx_tags(Path::new("ui.tsx"), source).0, ["button", "input"]);
    }
    #[test]
    fn unsupported_import_phases_are_never_synchronous_runtime_edges() {
        let source = include_str!("../tests/typescript/fixtures/deferred.ts");
        let (edges, gaps) = imports(Path::new("deferred.ts"), source);
        assert!(edges.is_empty());
        assert_eq!(gaps, [1]);
    }
    #[test]
    fn malformed_source_reports_original_sfc_lines() {
        for path in ["app.vue", "app.svelte"] {
            let (_, gaps) = imports(
                Path::new(path),
                "<template>é</template>\n<script lang=\"ts\">\nconst broken: = ;\n</script>",
            );
            assert!(gaps.contains(&3), "{path}: {gaps:?}");
        }
        let (_, gaps) = imports(Path::new("broken.ts"), "const broken = (");
        assert!(!gaps.is_empty());
    }
    #[test]
    fn commented_sfc_scripts_are_ignored() {
        let source = "<!-- <script>import './fake';</script> --><script setup lang=\"ts\">import './real';</script>";
        let (edges, _) = imports(Path::new("app.vue"), source);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].1, "./real");
    }
}
