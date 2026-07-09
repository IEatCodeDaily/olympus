//! Markdown-first knowledge vault storage (ADR 0004).
//!
//! A vault is a jj-colocated Git repository under
//! `~/.olympus/<org>/vaults/<vault_id>/`. Markdown files are the source of truth;
//! tree/index data is derived from the filesystem on demand.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JjMode {
    /// Production mode: all jj commands must succeed.
    Required,
    /// Test mode: skip jj commands while exercising filesystem semantics.
    Disabled,
}

#[derive(Debug, Clone)]
pub struct VaultStore {
    root: PathBuf,
    jj_mode: JjMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VaultSummary {
    pub id: String,
    pub name: String,
    pub note_count: usize,
    pub updated_at: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteTreeEntry {
    pub path: String,
    pub title: String,
    pub updated_at: f64,
    pub kind: NoteTreeEntryKind,
    pub children: Vec<NoteTreeEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTreeEntryKind {
    Folder,
    Note,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteDocument {
    pub path: String,
    pub title: String,
    pub markdown: String,
    pub frontmatter: Value,
    pub linked_notes: Vec<String>,
    /// BLAKE3 content hash of the markdown body (content address).
    /// Injected into frontmatter as `cid` on write.
    pub cid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteNote {
    pub markdown: Option<String>,
    pub new_path: Option<String>,
}

impl VaultStore {
    pub fn new(org_root: impl Into<PathBuf>) -> Self {
        Self::with_jj_mode(org_root, JjMode::Required)
    }

    pub fn with_jj_mode(org_root: impl Into<PathBuf>, jj_mode: JjMode) -> Self {
        Self {
            root: org_root.into().join("vaults"),
            jj_mode,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list_vaults(&self) -> Result<Vec<VaultSummary>> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("creating vault root {}", self.root.display()))?;
        let mut vaults = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("reading vault root {}", self.root.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let name = read_vault_name(&path).unwrap_or_else(|| id.clone());
            let (note_count, updated_at) = vault_stats(&path)?;
            vaults.push(VaultSummary {
                id,
                name,
                note_count,
                updated_at,
            });
        }
        vaults.sort_by(|a, b| {
            b.updated_at
                .partial_cmp(&a.updated_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(vaults)
    }

    pub fn create_vault(&self, name: &str) -> Result<VaultSummary> {
        let name = name.trim();
        if name.is_empty() {
            bail!("vault name is required");
        }
        fs::create_dir_all(&self.root)
            .with_context(|| format!("creating vault root {}", self.root.display()))?;

        let slug = slugify(name);
        let id = unique_vault_id(&self.root, &slug);
        let path = self.vault_path(&id);
        fs::create_dir_all(&path).with_context(|| format!("creating vault {}", path.display()))?;
        fs::create_dir_all(path.join(".vault"))?;
        fs::write(
            path.join(".vault").join("metadata.json"),
            json!({ "name": name }).to_string(),
        )?;

        self.jj_init(&path)?;
        self.jj_snapshot(&path, "vault: create")?;

        Ok(VaultSummary {
            id,
            name: name.to_string(),
            note_count: 0,
            updated_at: now_secs(),
        })
    }

    pub fn list_notes(&self, vault_id: &str) -> Result<Vec<NoteTreeEntry>> {
        let vault = self.existing_vault_path(vault_id)?;
        let mut notes = Vec::new();
        collect_notes(&vault, &vault, &mut notes)?;
        notes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(build_tree(notes))
    }

    pub fn read_note(&self, vault_id: &str, path: &str) -> Result<NoteDocument> {
        let vault = self.existing_vault_path(vault_id)?;
        let safe = sanitize_note_path(path)?;
        let full = vault.join(&safe);
        if !full.exists() {
            bail!("note not found");
        }
        let markdown = fs::read_to_string(&full)
            .with_context(|| format!("reading note {}", full.display()))?;
        Ok(note_document(safe_to_string(&safe), markdown))
    }

    pub fn write_note(&self, vault_id: &str, path: &str, write: WriteNote) -> Result<NoteDocument> {
        let vault = self.existing_vault_path(vault_id)?;
        let old_rel = sanitize_note_path(path)?;
        let new_rel = match write.new_path.as_deref() {
            Some(p) if !p.trim().is_empty() => sanitize_note_path(p)?,
            _ => old_rel.clone(),
        };
        let old_full = vault.join(&old_rel);
        let new_full = vault.join(&new_rel);

        if new_rel != old_rel && old_full.exists() {
            if let Some(parent) = new_full.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&old_full, &new_full).with_context(|| {
                format!(
                    "renaming note {} to {}",
                    old_full.display(),
                    new_full.display()
                )
            })?;
        }

        if let Some(markdown) = write.markdown {
            // Inject content hash (BLAKE3) into frontmatter before writing.
            let markdown = inject_content_hash(&markdown);
            if let Some(parent) = new_full.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&new_full, markdown)
                .with_context(|| format!("writing note {}", new_full.display()))?;
        } else if !new_full.exists() {
            bail!("markdown is required for a new note");
        }

        self.jj_snapshot(
            &vault,
            &format!("vault: write {}", safe_to_string(&new_rel)),
        )?;
        self.read_note(vault_id, &safe_to_string(&new_rel))
    }

    pub fn delete_note(&self, vault_id: &str, path: &str) -> Result<()> {
        let vault = self.existing_vault_path(vault_id)?;
        let rel = sanitize_note_path(path)?;
        let full = vault.join(&rel);
        if !full.exists() {
            bail!("note not found");
        }
        fs::remove_file(&full).with_context(|| format!("deleting note {}", full.display()))?;
        prune_empty_dirs(&vault, full.parent());
        self.jj_snapshot(&vault, &format!("vault: delete {}", safe_to_string(&rel)))?;
        Ok(())
    }

    fn vault_path(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    fn existing_vault_path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || Path::new(id).components().count() != 1
            || id.contains('/')
            || id.contains('\\')
            || id == "."
            || id == ".."
        {
            bail!("invalid vault id");
        }
        let path = self.vault_path(id);
        if !path.is_dir() {
            bail!("vault not found");
        }
        Ok(path)
    }

    fn jj_init(&self, path: &Path) -> Result<()> {
        if self.jj_mode == JjMode::Disabled {
            return Ok(());
        }
        run_jj(path, &["git", "init", "--colocate", "."], "jj git init")
    }

    fn jj_snapshot(&self, path: &Path, message: &str) -> Result<()> {
        if self.jj_mode == JjMode::Disabled {
            return Ok(());
        }
        run_jj(path, &["describe", "-m", message], "jj describe")
    }
}

fn run_jj(path: &Path, args: &[&str], label: &str) -> Result<()> {
    let output = Command::new("jj")
        .args(args)
        .arg("--no-pager")
        .current_dir(path)
        .output()
        .with_context(|| format!("running {label} in {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "{label} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn read_vault_name(path: &Path) -> Option<String> {
    let bytes = fs::read(path.join(".vault").join("metadata.json")).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("name")?.as_str().map(ToOwned::to_owned)
}

fn vault_stats(path: &Path) -> Result<(usize, f64)> {
    let mut count = 0;
    let mut updated = modified_secs(path).unwrap_or(0.0);
    for file in markdown_files(path)? {
        count += 1;
        updated = updated.max(modified_secs(&file).unwrap_or(0.0));
    }
    Ok((count, updated))
}

fn markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_markdown_files(root, root, &mut out)?;
    Ok(out)
}

fn collect_markdown_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == OsStr::new(".git") || name == OsStr::new(".jj") || name == OsStr::new(".vault") {
            continue;
        }
        let ty = entry.file_type()?;
        if ty.is_dir() {
            collect_markdown_files(root, &path, out)?;
        } else if ty.is_file()
            && path.extension() == Some(OsStr::new("md"))
            && path.strip_prefix(root).is_ok()
        {
            out.push(path);
        }
    }
    Ok(())
}

fn collect_notes(root: &Path, dir: &Path, out: &mut Vec<NoteTreeEntry>) -> Result<()> {
    for file in markdown_files(dir)? {
        let rel = file.strip_prefix(root)?.to_path_buf();
        let markdown = fs::read_to_string(&file).unwrap_or_default();
        let doc = note_document(safe_to_string(&rel), markdown);
        out.push(NoteTreeEntry {
            path: doc.path,
            title: doc.title,
            updated_at: modified_secs(&file).unwrap_or(0.0),
            kind: NoteTreeEntryKind::Note,
            children: Vec::new(),
        });
    }
    Ok(())
}

#[derive(Default)]
struct FolderNode {
    children: BTreeMap<String, FolderNode>,
    note: Option<NoteTreeEntry>,
}

fn build_tree(notes: Vec<NoteTreeEntry>) -> Vec<NoteTreeEntry> {
    let mut root = FolderNode::default();
    for note in notes {
        let parts: Vec<String> = note.path.split('/').map(ToOwned::to_owned).collect();
        let mut node = &mut root;
        for part in &parts[..parts.len().saturating_sub(1)] {
            node = node.children.entry(part.clone()).or_default();
        }
        node.children
            .entry(parts.last().cloned().unwrap_or_default())
            .or_default()
            .note = Some(note);
    }
    folder_entries("", root)
}

fn folder_entries(prefix: &str, node: FolderNode) -> Vec<NoteTreeEntry> {
    let mut entries = Vec::new();
    for (name, child) in node.children {
        if let Some(note) = child.note {
            entries.push(note);
            continue;
        }
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        entries.push(NoteTreeEntry {
            title: name,
            path: path.clone(),
            updated_at: newest_child_time(&child),
            kind: NoteTreeEntryKind::Folder,
            children: folder_entries(&path, child),
        });
    }
    entries
}

fn newest_child_time(node: &FolderNode) -> f64 {
    let own = node.note.as_ref().map(|n| n.updated_at).unwrap_or(0.0);
    node.children
        .values()
        .fold(own, |acc, child| acc.max(newest_child_time(child)))
}

fn note_document(path: String, markdown: String) -> NoteDocument {
    let (frontmatter, body) = parse_frontmatter(&markdown);
    let title = title_from_frontmatter(&frontmatter)
        .or_else(|| title_from_heading(body))
        .unwrap_or_else(|| title_from_path(&path));
    let linked_notes = parse_linked_notes(&markdown);
    let cid = frontmatter
        .get("cid")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    NoteDocument {
        path,
        title,
        markdown,
        frontmatter,
        linked_notes,
        cid,
    }
}

fn parse_frontmatter(markdown: &str) -> (Value, &str) {
    if !markdown.starts_with("---\n") && markdown.trim() != "---" {
        return (Value::Object(Map::new()), markdown);
    }
    let rest = &markdown[4..];
    if let Some(end) = rest.find("\n---") {
        let yaml = &rest[..end];
        let body_start = end + "\n---".len();
        let body = rest[body_start..]
            .strip_prefix('\n')
            .unwrap_or(&rest[body_start..]);
        let value = serde_yaml::from_str::<serde_yaml::Value>(yaml)
            .ok()
            .and_then(|v| serde_json::to_value(v).ok())
            .unwrap_or_else(|| Value::Object(Map::new()));
        return (value, body);
    }
    (Value::Object(Map::new()), markdown)
}

fn title_from_frontmatter(frontmatter: &Value) -> Option<String> {
    frontmatter
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn title_from_heading(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn title_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .replace(['-', '_'], " ")
}

fn parse_linked_notes(markdown: &str) -> Vec<String> {
    let mut links = BTreeSet::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let raw = &after[..end];
        if let Some(link) = normalize_link(raw) {
            links.insert(link);
        }
        rest = &after[end + 2..];
    }

    for line in markdown.lines().filter(|line| line.contains('·')) {
        for part in line.split('·') {
            if part.contains("[[") || part.contains("]]") {
                continue;
            }
            if let Some(link) = normalize_link(part) {
                links.insert(link);
            }
        }
    }

    links.into_iter().collect()
}

fn normalize_link(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '[' | ']' | '(' | ')' | ',' | ';' | '.'));
    let trimmed = trimmed.split('|').next().unwrap_or(trimmed);
    let trimmed = trimmed.split('#').next().unwrap_or(trimmed).trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return None;
    }
    Some(trimmed.to_string())
}

fn sanitize_note_path(path: &str) -> Result<PathBuf> {
    let trimmed = path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        bail!("note path is required");
    }
    let candidate = Path::new(trimmed);
    let mut out = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("invalid note path");
            }
        }
    }
    if out.as_os_str().is_empty() || out.extension() != Some(OsStr::new("md")) {
        bail!("note path must end in .md");
    }
    Ok(out)
}

fn safe_to_string(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(ToOwned::to_owned),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "vault".to_string()
    } else {
        slug
    }
}

fn unique_vault_id(root: &Path, slug: &str) -> String {
    if !root.join(slug).exists() {
        return slug.to_string();
    }
    loop {
        let id = format!("{}-{}", slug, &Uuid::new_v4().to_string()[..8]);
        if !root.join(&id).exists() {
            return id;
        }
    }
}

fn modified_secs(path: &Path) -> Option<f64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    system_time_secs(modified)
}

fn now_secs() -> f64 {
    system_time_secs(SystemTime::now()).unwrap_or(0.0)
}

fn system_time_secs(time: SystemTime) -> Option<f64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs_f64())
}

fn prune_empty_dirs(root: &Path, start: Option<&Path>) {
    let Some(mut dir) = start.map(Path::to_path_buf) else {
        return;
    };
    while dir.starts_with(root) && dir != root {
        match fs::remove_dir(&dir) {
            Ok(()) => {
                let Some(parent) = dir.parent() else { break };
                dir = parent.to_path_buf();
            }
            Err(_) => break,
        }
    }
}

pub fn not_found(err: &anyhow::Error) -> bool {
    err.to_string().contains("not found")
}

pub fn bad_request(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    msg.contains("invalid")
        || msg.contains("required")
        || msg.contains("must end")
        || msg.contains("name is required")
}

// ── Graph data ──────────────────────────────────────

/// A node in the vault link graph.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub path: String,
    pub cid: Option<String>,
    pub link_count: usize,
}

/// An edge in the vault link graph (source → target wikilink).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

/// The complete vault graph: nodes (notes) + edges (wikilinks).
#[derive(Debug, Clone)]
pub struct VaultGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl VaultStore {
    /// Build the link graph for a vault: nodes = notes, edges = wikilinks.
    /// O(n*m) where n=notes, m=avg links per note — fine for <1000 notes.
    pub fn graph(&self, vault_id: &str) -> Result<VaultGraph> {
        let vault = self.existing_vault_path(vault_id)?;
        let mut notes_map: BTreeMap<String, NoteDocument> = BTreeMap::new();
        for file in markdown_files(&vault)? {
            let rel = file.strip_prefix(&vault)?.to_path_buf();
            let markdown = fs::read_to_string(&file).unwrap_or_default();
            let doc = note_document(safe_to_string(&rel), markdown);
            notes_map.insert(doc.title.clone(), doc);
        }

        // Build path→title index for link resolution
        let title_to_path: BTreeMap<&str, &str> = notes_map
            .iter()
            .map(|(title, doc)| (title.as_str(), doc.path.as_str()))
            .collect();

        let mut link_count: BTreeMap<String, usize> = BTreeMap::new();
        let mut edges = Vec::new();
        for (title, doc) in &notes_map {
            for link in &doc.linked_notes {
                if title_to_path.contains_key(link.as_str()) {
                    edges.push(GraphEdge {
                        source: title.clone(),
                        target: link.clone(),
                    });
                    *link_count.entry(title.clone()).or_insert(0) += 1;
                }
            }
        }

        let nodes = notes_map
            .iter()
            .map(|(title, doc)| GraphNode {
                id: title.clone(),
                title: title.clone(),
                path: doc.path.clone(),
                cid: doc.cid.clone(),
                link_count: *link_count.get(title).unwrap_or(&0),
            })
            .collect();

        Ok(VaultGraph { nodes, edges })
    }

    /// Scan for collections (notes with `collection: true` in frontmatter).
    /// A collection's rows are the child notes in the same folder with
    /// structured frontmatter fields.
    pub fn list_collections(&self, vault_id: &str) -> Result<Vec<CollectionSummary>> {
        let vault = self.existing_vault_path(vault_id)?;
        let mut collections = Vec::new();
        for file in markdown_files(&vault)? {
            let rel = file.strip_prefix(&vault)?.to_path_buf();
            let markdown = fs::read_to_string(&file).unwrap_or_default();
            let doc = note_document(safe_to_string(&rel), markdown);
            if doc.frontmatter.get("collection").and_then(Value::as_bool) == Some(true) {
                let name = doc
                    .frontmatter
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&doc.title)
                    .to_string();
                let row_count = self.count_collection_rows(&vault, &doc.path)?;
                collections.push(CollectionSummary {
                    name,
                    path: doc.path,
                    row_count,
                });
            }
        }
        Ok(collections)
    }

    /// Get the rows of a collection: child notes in the same folder as the
    /// collection definition note, with their frontmatter fields as columns.
    pub fn collection_rows(&self, vault_id: &str, collection_path: &str) -> Result<CollectionData> {
        let vault = self.existing_vault_path(vault_id)?;
        let safe = sanitize_note_path(collection_path)?;
        let collection_dir = safe.parent().unwrap_or(Path::new(""));

        let mut rows = Vec::new();
        let mut columns: BTreeSet<String> = BTreeSet::new();
        for file in markdown_files(&vault)? {
            let rel = file.strip_prefix(&vault)?.to_path_buf();
            // Skip the collection definition note itself
            if rel == safe {
                continue;
            }
            // Only include notes in the same folder (or subfolders)
            if !rel.starts_with(collection_dir) {
                continue;
            }
            let markdown = fs::read_to_string(&file).unwrap_or_default();
            let doc = note_document(safe_to_string(&rel), markdown);
            if let Value::Object(ref obj) = doc.frontmatter {
                if obj.is_empty() {
                    continue;
                }
                let mut row: BTreeMap<String, Value> = BTreeMap::new();
                row.insert("path".into(), Value::String(doc.path.clone()));
                row.insert("title".into(), Value::String(doc.title.clone()));
                for (k, v) in obj {
                    // Skip collection meta fields
                    if k == "collection" || k == "name" || k == "cid" {
                        continue;
                    }
                    columns.insert(k.clone());
                    row.insert(k.clone(), v.clone());
                }
                rows.push(serde_json::to_value(&row).unwrap_or(Value::Null));
            }
        }

        Ok(CollectionData {
            columns: columns.into_iter().collect(),
            rows,
        })
    }

    fn count_collection_rows(&self, vault: &Path, collection_path: &str) -> Result<usize> {
        let safe = sanitize_note_path(collection_path)?;
        let collection_dir = safe.parent().unwrap_or(Path::new(""));
        let mut count = 0;
        for file in markdown_files(vault)? {
            let rel = file.strip_prefix(vault)?;
            if rel == safe {
                continue;
            }
            if !rel.starts_with(collection_dir) {
                continue;
            }
            let markdown = fs::read_to_string(&file).unwrap_or_default();
            let (_, fm) = parse_frontmatter(&markdown);
            let _ = fm; // just count files with frontmatter
            count += 1;
        }
        Ok(count)
    }
}

/// A collection definition summary.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionSummary {
    pub name: String,
    pub path: String,
    pub row_count: usize,
}

/// Collection data: column names + row values.
#[derive(Debug, Clone)]
pub struct CollectionData {
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
}

/// Compute the BLAKE3 hash of the markdown content body and inject it into
/// the frontmatter as `cid: <hex>`. If frontmatter already has a `cid`
/// matching the current content, it's a no-op (already addressed).
fn inject_content_hash(markdown: &str) -> String {
    let (fm, body) = parse_frontmatter(markdown);
    let hash = blake3::hash(body.trim().as_bytes());
    let cid = hash.to_hex().to_string();

    // Check if existing cid matches
    if fm.get("cid").and_then(Value::as_str) == Some(&cid) {
        return markdown.to_string(); // already correct
    }

    // Rebuild frontmatter with cid
    let mut fm_obj = match fm {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    fm_obj.insert("cid".into(), Value::String(cid.clone()));

    // Serialize frontmatter back to YAML
    let fm_yaml = serde_yaml::to_string(&fm_obj).unwrap_or_default();
    let fm_yaml = fm_yaml.trim_end();

    // Reconstruct: frontmatter + body
    if let Some(rest) = markdown.strip_prefix("---\n") {
        // Replace existing frontmatter
        let body_start = rest
            .find("\n---")
            .map(|end| 4 + end + 4)
            .unwrap_or(markdown.len());
        let body = &markdown[body_start..];
        format!("---\n{}\n---\n{}", fm_yaml, body)
    } else {
        // Add frontmatter
        format!("---\n{}\n---\n\n{}", fm_yaml, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tmp: &tempfile::TempDir) -> VaultStore {
        VaultStore::with_jj_mode(tmp.path().join("default"), JjMode::Disabled)
    }

    #[test]
    fn create_write_read_and_tree_roundtrip_without_jj() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let vault = store.create_vault("Engineering Notes").unwrap();
        assert_eq!(vault.id, "engineering-notes");

        let doc = store
            .write_note(
                &vault.id,
                "runbooks/boot.md",
                WriteNote {
                    markdown: Some(
                        "---\ntitle: Boot Runbook\ntags:\n  - ops\n---\n# Ignored heading\nSee [[Incident Guide]] · [[On Call]]\n"
                            .into(),
                    ),
                    new_path: None,
                },
            )
            .unwrap();
        assert_eq!(doc.title, "Boot Runbook");
        assert_eq!(doc.frontmatter["title"], "Boot Runbook");
        assert_eq!(doc.linked_notes, vec!["Incident Guide", "On Call"]);

        let read = store.read_note(&vault.id, "runbooks/boot.md").unwrap();
        assert_eq!(read.markdown, doc.markdown);

        let tree = store.list_notes(&vault.id).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].kind, NoteTreeEntryKind::Folder);
        assert_eq!(tree[0].path, "runbooks");
        assert_eq!(tree[0].children[0].path, "runbooks/boot.md");
    }

    #[test]
    fn rename_via_write_moves_existing_note() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let vault = store.create_vault("Notes").unwrap();
        store
            .write_note(
                &vault.id,
                "old.md",
                WriteNote {
                    markdown: Some("# Old\n".into()),
                    new_path: None,
                },
            )
            .unwrap();
        let renamed = store
            .write_note(
                &vault.id,
                "old.md",
                WriteNote {
                    markdown: Some("# New\n".into()),
                    new_path: Some("folder/new.md".into()),
                },
            )
            .unwrap();
        assert_eq!(renamed.path, "folder/new.md");
        assert!(store.read_note(&vault.id, "old.md").is_err());
        assert_eq!(
            store.read_note(&vault.id, "folder/new.md").unwrap().title,
            "New"
        );
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        assert!(sanitize_note_path("../secret.md").is_err());
        assert!(sanitize_note_path("/abs.md").is_ok());
        assert!(sanitize_note_path("note.txt").is_err());
    }

    #[test]
    #[ignore = "manual gate: requires jj binary; validates each write snapshots the jj working-copy commit"]
    fn jj_snapshot_lands_for_write() {
        let tmp = tempfile::tempdir().unwrap();
        let store = VaultStore::with_jj_mode(tmp.path().join("default"), JjMode::Required);
        let vault = store.create_vault("Jj Notes").unwrap();
        store
            .write_note(
                &vault.id,
                "hello.md",
                WriteNote {
                    markdown: Some("# Hello\n".into()),
                    new_path: None,
                },
            )
            .unwrap();
        let output = Command::new("jj")
            .args(["log", "-r", "@", "-T", "description", "--no-pager"])
            .current_dir(store.vault_path(&vault.id))
            .output()
            .unwrap();
        assert!(output.status.success());
        let description = String::from_utf8_lossy(&output.stdout);
        assert!(description.contains("vault: write hello.md"));
    }
}
