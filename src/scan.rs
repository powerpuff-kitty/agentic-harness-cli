use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct Inventory {
    pub files: Vec<PathBuf>,
    pub skipped: Vec<Value>,
    pub errors: Vec<String>,
}
impl Inventory {
    pub fn report(&self) -> Value {
        json!({"complete":self.errors.is_empty(),"files":self.files.len(),"skipped":self.skipped,"errors":self.errors,"policy":"repository-local .gitignore/.ignore/.ahignore; no symlink traversal; generated/vendor directories excluded"})
    }
}
pub fn require_directory(root: &Path) -> Result<(), String> {
    if !root.is_dir() {
        return Err(format!("target is not a directory: {}", root.display()));
    }
    std::fs::read_dir(root)
        .map(|_| ())
        .map_err(|e| format!("cannot read target: {e}"))
}
pub fn excluded(name: &str) -> bool {
    [
        ".git",
        "node_modules",
        "vendor",
        "dist",
        "dist-ssr",
        "build",
        ".next",
        ".nuxt",
        "target",
        ".venv",
        "venv",
        "coverage",
        "upstream",
        ".wrangler",
        ".logs",
        ".secrets",
        "backups",
        "tmp",
        "test-results",
        "playwright-report",
    ]
    .contains(&name)
}
fn collect(root: &Path) -> Inventory {
    let mut result = Inventory::default();
    if let Err(e) = require_directory(root) {
        result.errors.push(e);
        return result;
    }
    let skipped = Arc::new(Mutex::new(Vec::new()));
    let filtered = skipped.clone();
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(false)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .add_custom_ignore_filename(".ahignore")
        .sort_by_file_path(|a, b| a.cmp(b));
    builder.filter_entry(move |entry| {
        if entry.depth() == 0 {
            return true;
        }
        let reject = entry.file_type().is_some_and(|t| t.is_symlink())
            || (entry.file_type().is_some_and(|t| t.is_dir())
                && excluded(&entry.file_name().to_string_lossy()));
        if reject {
            filtered
                .lock()
                .unwrap()
                .push(json!({"path":entry.path(),"reason":"symlink or excluded directory"}));
        }
        !reject
    });
    for entry in builder.build() {
        match entry {
            Ok(e) => {
                if let Some(error) = e.error() {
                    result.errors.push(error.to_string());
                }
                if e.file_type().is_some_and(|t| t.is_file()) {
                    result.files.push(e.into_path());
                }
            }
            Err(e) => result.errors.push(e.to_string()),
        }
    }
    result.skipped = skipped.lock().unwrap().clone();
    result.files.sort();
    result
}
pub fn files(root: &Path) -> Vec<PathBuf> {
    inventory(root).files
}
pub fn product(root: &Path, path: &Path) -> bool {
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    !rel.split('/').any(|s| {
        [
            ".agentic",
            ".agents",
            ".claude",
            ".github",
            ".codex",
            "skills",
            "docs",
            "scripts",
            "tests",
            "test",
            "__tests__",
            "fixtures",
            "examples",
            "stories",
        ]
        .contains(&s)
    }) && !rel.contains(".test.")
        && !rel.contains(".spec.")
        && !rel.contains(".stories.")
        && !rel.ends_with(".d.ts")
}
pub fn read(root: &Path, path: &Path, limit: u64) -> Result<String, String> {
    // Also guard explicit context/config paths that bypass the inventory.
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    let resolved = path.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&canonical) {
        return Err("path escapes target".into());
    }
    let m = std::fs::metadata(&resolved).map_err(|e| e.to_string())?;
    if !m.is_file() || m.len() > limit {
        return Err("not a regular file or exceeds size limit".into());
    }
    std::fs::read_to_string(resolved).map_err(|e| e.to_string())
}

thread_local! {
    static CACHE:std::cell::RefCell<Option<std::collections::BTreeMap<PathBuf,Inventory>>>=const {std::cell::RefCell::new(None)};
}
pub fn begin() {
    CACHE.with(|c| *c.borrow_mut() = Some(std::collections::BTreeMap::new()));
}
pub fn inventory(root: &Path) -> Inventory {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let found = CACHE.with(|c| c.borrow().as_ref().and_then(|m| m.get(&canonical).cloned()));
    let mut found = found.unwrap_or_else(|| {
        let found = collect(&canonical);
        CACHE.with(|c| {
            if let Some(m) = c.borrow_mut().as_mut() {
                m.insert(canonical.clone(), found.clone());
            }
        });
        found
    });
    if root != canonical {
        found.files = found
            .files
            .iter()
            .map(|p| root.join(p.strip_prefix(&canonical).unwrap_or(p)))
            .collect();
    }
    found
}

/// Atomic replacement for explicit repository-local generated artifacts.
pub fn write(root: &Path, relative: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::{Error, Write};
    let destination = root.join(relative);
    for path in destination.ancestors().take_while(|p| *p != root) {
        if path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(Error::other("symlink destination is not supported"));
        }
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Error::other("missing destination parent"))?;
    std::fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(Error::other("destination escapes repository"));
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(destination).map_err(Error::other)?;
    Ok(())
}
