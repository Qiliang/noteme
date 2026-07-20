use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

use crate::settings::{self, SettingsState};

const FILE_TYPE_OPAQUE: u16 = 0;
const FILE_TYPE_TEXT: u16 = 1;
const FILE_TYPE_MARKDOWN: u16 = 2;

/// Soft cap for a single directory listing to keep the UI responsive.
const MAX_DIR_ENTRIES: usize = 5000;

/// Soft caps for orphan-asset cleanup scans.
const MAX_WALK_ENTRIES: usize = 50_000;
const MAX_DOC_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub ctime_ms: u64,
    pub mtime_ms: u64,
    pub atime_ms: u64,
    pub file_type: u16,
    pub content_hash: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResult {
    pub entries: Vec<FileInfo>,
    pub truncated: bool,
}

fn system_time_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn guess_file_type(name: &str) -> u16 {
    let lower = name.to_lowercase();
    if lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".mdx") {
        FILE_TYPE_MARKDOWN
    } else if lower.ends_with(".txt")
        || lower.ends_with(".text")
        || lower.ends_with(".json")
        || lower.ends_with(".csv")
        || lower.ends_with(".yml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".toml")
        || lower.ends_with(".rs")
        || lower.ends_with(".js")
        || lower.ends_with(".ts")
        || lower.ends_with(".css")
        || lower.ends_with(".html")
        || lower.ends_with(".excalidraw")
    {
        FILE_TYPE_TEXT
    } else {
        FILE_TYPE_OPAQUE
    }
}

fn should_skip_name(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "target" | ".DS_Store")
}

/// Normalize a relative path: no absolute, no `..` escape, forward slashes.
pub fn normalize_rel(rel: &str) -> Result<String, String> {
    if rel.is_empty() {
        return Err("路径不能为空".to_string());
    }
    if rel.contains('\0') {
        return Err("路径包含非法字符".to_string());
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err("不允许绝对路径".to_string());
    }
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_string_lossy();
                if s == "." || s == ".." {
                    return Err("路径段非法".to_string());
                }
                parts.push(s.into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("路径越界".to_string());
                }
            }
            _ => return Err("路径非法".to_string()),
        }
    }
    if parts.is_empty() {
        return Err("路径不能为空".to_string());
    }
    Ok(parts.join("/"))
}

pub fn resolve_under_root(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let norm = normalize_rel(rel)?;
    let joined = root.join(&norm);
    if joined.exists() {
        let canon_root = root
            .canonicalize()
            .map_err(|e| format!("解析文档根失败: {e}"))?;
        let canon = joined
            .canonicalize()
            .map_err(|e| format!("解析路径失败: {e}"))?;
        if !canon.starts_with(&canon_root) {
            return Err("路径越界".to_string());
        }
        return Ok(canon);
    }
    let mut check = PathBuf::new();
    for c in Path::new(&norm).components() {
        check.push(c);
        let candidate = root.join(&check);
        if candidate.exists() {
            let canon_root = root
                .canonicalize()
                .map_err(|e| format!("解析文档根失败: {e}"))?;
            let canon = candidate
                .canonicalize()
                .map_err(|e| format!("解析路径失败: {e}"))?;
            if !canon.starts_with(&canon_root) {
                return Err("路径越界".to_string());
            }
        }
    }
    Ok(joined)
}

fn meta_to_info(rel: String, meta: &fs::Metadata, is_dir: bool) -> FileInfo {
    let mtime = meta.modified().ok().map(system_time_ms).unwrap_or(0);
    let atime = meta.accessed().ok().map(system_time_ms).unwrap_or(mtime);
    let ctime = meta.created().ok().map(system_time_ms).unwrap_or(mtime);
    FileInfo {
        file_type: if is_dir {
            FILE_TYPE_OPAQUE
        } else {
            guess_file_type(&rel)
        },
        name: rel,
        size: if is_dir { 0 } else { meta.len() },
        ctime_ms: ctime,
        mtime_ms: mtime,
        atime_ms: atime,
        content_hash: String::new(),
        is_dir,
    }
}

fn dir_for_prefix(root: &Path, prefix: &str) -> Result<PathBuf, String> {
    let prefix = prefix.trim_start_matches('/').trim_end_matches('/');
    if prefix.is_empty() {
        return Ok(root.to_path_buf());
    }
    let path = resolve_under_root(root, prefix)?;
    if !path.is_dir() {
        return Err("前缀不是目录".to_string());
    }
    Ok(path)
}

/// List a single directory (not recursive). `prefix` is a relative directory
/// path, with or without trailing `/` (empty = document root).
#[tauri::command]
pub fn fs_list(state: State<'_, SettingsState>, prefix: String) -> Result<ListResult, String> {
    let root = settings::document_root(&state)?;
    let dir = dir_for_prefix(&root, &prefix)?;
    let prefix_norm = {
        let p = prefix.trim_start_matches('/').trim_end_matches('/');
        if p.is_empty() {
            String::new()
        } else {
            format!("{p}/")
        }
    };

    let mut entries = Vec::new();
    let mut truncated = false;
    let read = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {e}"))?;
    for entry in read {
        if entries.len() >= MAX_DIR_ENTRIES {
            truncated = true;
            break;
        }
        let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
        let os_name = entry.file_name();
        let name = os_name.to_string_lossy();
        if should_skip_name(&name) {
            continue;
        }
        let ft = entry
            .file_type()
            .map_err(|e| format!("读取文件类型失败: {e}"))?;
        let is_dir = ft.is_dir();
        if !is_dir && !ft.is_file() {
            continue;
        }
        let rel = format!("{prefix_norm}{name}");
        let meta = entry
            .metadata()
            .map_err(|e| format!("读取元数据失败: {e}"))?;
        entries.push(meta_to_info(rel, &meta, is_dir));
    }

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    Ok(ListResult {
        entries,
        truncated,
    })
}

#[tauri::command]
pub fn fs_stat(state: State<'_, SettingsState>, name: String) -> Result<FileInfo, String> {
    let root = settings::document_root(&state)?;
    let path = resolve_under_root(&root, &name)?;
    let meta = fs::metadata(&path).map_err(|e| format!("stat 失败: {e}"))?;
    let is_dir = meta.is_dir();
    if !is_dir && !meta.is_file() {
        return Err("不是文件或目录".to_string());
    }
    let rel = normalize_rel(&name)?;
    Ok(meta_to_info(rel, &meta, is_dir))
}

#[tauri::command]
pub fn fs_read(state: State<'_, SettingsState>, name: String) -> Result<String, String> {
    let root = settings::document_root(&state)?;
    let path = resolve_under_root(&root, &name)?;
    fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))
}

#[tauri::command]
pub fn fs_read_bytes(state: State<'_, SettingsState>, name: String) -> Result<Vec<u8>, String> {
    let root = settings::document_root(&state)?;
    let path = resolve_under_root(&root, &name)?;
    fs::read(&path).map_err(|e| format!("读取失败: {e}"))
}

#[tauri::command]
pub fn fs_write(
    state: State<'_, SettingsState>,
    name: String,
    content: String,
) -> Result<(), String> {
    let root = settings::document_root(&state)?;
    let path = resolve_under_root(&root, &name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    fs::write(&path, content.as_bytes()).map_err(|e| format!("写入失败: {e}"))
}

#[tauri::command]
pub fn fs_write_bytes(
    state: State<'_, SettingsState>,
    name: String,
    data: Vec<u8>,
) -> Result<FileInfo, String> {
    let root = settings::document_root(&state)?;
    let path = resolve_under_root(&root, &name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    fs::write(&path, &data).map_err(|e| format!("写入失败: {e}"))?;
    let meta = fs::metadata(&path).map_err(|e| format!("写入后 stat 失败: {e}"))?;
    let rel = normalize_rel(&name)?;
    Ok(meta_to_info(rel, &meta, false))
}

#[tauri::command]
pub fn fs_delete(state: State<'_, SettingsState>, name: String) -> Result<(), String> {
    let root = settings::document_root(&state)?;
    let path = resolve_under_root(&root, &name)?;
    if !path.exists() {
        return Err("文件不存在".to_string());
    }
    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| format!("删除目录失败: {e}"))?;
    } else {
        fs::remove_file(&path).map_err(|e| format!("删除失败: {e}"))?;
    }
    let mut parent = path.parent().map(|p| p.to_path_buf());
    while let Some(dir) = parent {
        if dir == root {
            break;
        }
        match fs::remove_dir(&dir) {
            Ok(()) => parent = dir.parent().map(|p| p.to_path_buf()),
            Err(_) => break,
        }
    }
    Ok(())
}

#[tauri::command]
pub fn fs_rename(
    state: State<'_, SettingsState>,
    old: String,
    new: String,
) -> Result<(), String> {
    let root = settings::document_root(&state)?;
    let from = resolve_under_root(&root, &old)?;
    let to = resolve_under_root(&root, &new)?;
    if !from.exists() {
        return Err("源文件不存在".to_string());
    }
    if to.exists() {
        return Err("目标已存在".to_string());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    fs::rename(&from, &to).map_err(|e| format!("重命名失败: {e}"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanAssetsResult {
    pub orphans: Vec<FileInfo>,
    pub asset_count: usize,
    pub doc_count: usize,
    pub truncated: bool,
}

fn is_scan_doc(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".md")
        || lower.ends_with(".markdown")
        || lower.ends_with(".mdx")
        || lower.ends_with(".txt")
        || lower.ends_with(".html")
        || lower.ends_with(".htm")
}

fn note_dir_rel(note_path: &str) -> String {
    match note_path.rfind('/') {
        Some(i) => note_path[..=i].to_string(),
        None => String::new(),
    }
}

fn resolve_href(note_path: &str, href: &str) -> Option<String> {
    let mut h = href.trim();
    if h.is_empty() {
        return None;
    }
    if let Some(i) = h.find(['?', '#']) {
        h = &h[..i];
    }
    if h.is_empty()
        || h.starts_with('#')
        || h.starts_with("data:")
        || h.starts_with("blob:")
        || h.contains("://")
    {
        return None;
    }
    // Strip accidental app-origin absolutization.
    for prefix in [
        "http://localhost/",
        "http://127.0.0.1/",
        "https://localhost/",
        "https://127.0.0.1/",
    ] {
        if let Some(rest) = h.strip_prefix(prefix) {
            h = rest;
            break;
        }
    }
    let root_abs = h.starts_with('/');
    let cleaned = h.trim_start_matches('/');
    if cleaned.is_empty() {
        return None;
    }
    let base = if root_abs {
        String::new()
    } else {
        note_dir_rel(note_path)
    };
    let joined = format!("{base}{cleaned}");
    let mut parts: Vec<&str> = Vec::new();
    for p in joined.split('/') {
        if p.is_empty() || p == "." {
            continue;
        }
        if p == ".." {
            if parts.pop().is_none() {
                return None;
            }
        } else {
            parts.push(p);
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn push_href(out: &mut HashSet<String>, raw: &str) {
    let t = raw
        .trim()
        .trim_matches(|c| c == '<' || c == '>')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c| c == '"' || c == '\'');
    if !t.is_empty() {
        out.insert(t.to_string());
    }
}

fn extract_hrefs(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b')' {
                j += 1;
            }
            if j < bytes.len() {
                push_href(&mut out, &text[start..j]);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }

    for (needle, end_ch) in [
        ("src=\"", '"'),
        ("src='", '\''),
        ("href=\"", '"'),
        ("href='", '\''),
        ("SRC=\"", '"'),
        ("HREF=\"", '"'),
    ] {
        let mut from = 0;
        while let Some(pos) = text[from..].find(needle) {
            let abs = from + pos + needle.len();
            if let Some(end) = text[abs..].find(end_ch) {
                push_href(&mut out, &text[abs..abs + end]);
                from = abs + end + 1;
            } else {
                break;
            }
        }
    }

    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.contains(' ') || t.contains('\t') {
            continue;
        }
        if t.contains(".assets/") || t.ends_with(".excalidraw") {
            push_href(&mut out, t);
        }
    }
    out
}

struct WalkAcc {
    assets: Vec<(String, fs::Metadata)>,
    docs: Vec<String>,
    entries: usize,
    truncated: bool,
}

fn walk_docs_and_assets(root: &Path, rel_dir: &str, acc: &mut WalkAcc) {
    if acc.truncated {
        return;
    }
    let dir = if rel_dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel_dir)
    };
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in read.flatten() {
        if acc.entries >= MAX_WALK_ENTRIES {
            acc.truncated = true;
            return;
        }
        acc.entries += 1;
        let os_name = entry.file_name();
        let name = os_name.to_string_lossy();
        if should_skip_name(&name) {
            continue;
        }
        let rel = if rel_dir.is_empty() {
            name.to_string()
        } else {
            format!("{rel_dir}/{name}")
        };
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if name == ".assets" {
                collect_asset_files(root, &rel, acc);
            } else {
                walk_docs_and_assets(root, &rel, acc);
            }
        } else if ft.is_file() && is_scan_doc(&name) {
            acc.docs.push(rel);
        }
    }
}

fn collect_asset_files(root: &Path, assets_rel: &str, acc: &mut WalkAcc) {
    let dir = root.join(assets_rel);
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in read.flatten() {
        if acc.entries >= MAX_WALK_ENTRIES {
            acc.truncated = true;
            return;
        }
        acc.entries += 1;
        let os_name = entry.file_name();
        let name = os_name.to_string_lossy();
        if should_skip_name(&name) || name.starts_with('.') {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let rel = format!("{assets_rel}/{name}");
        if ft.is_dir() {
            collect_asset_files(root, &rel, acc);
        } else if ft.is_file() {
            if let Ok(meta) = entry.metadata() {
                acc.assets.push((rel, meta));
            }
        }
    }
}

/// Find files under any `.assets/` directory that are not referenced by docs.
#[tauri::command]
pub fn fs_find_orphan_assets(
    state: State<'_, SettingsState>,
) -> Result<OrphanAssetsResult, String> {
    let root = settings::document_root(&state)?;
    let mut acc = WalkAcc {
        assets: Vec::new(),
        docs: Vec::new(),
        entries: 0,
        truncated: false,
    };
    walk_docs_and_assets(&root, "", &mut acc);

    let asset_count = acc.assets.len();
    let doc_count = acc.docs.len();
    let asset_set: HashSet<String> = acc.assets.iter().map(|(p, _)| p.clone()).collect();

    let mut basename_index: HashMap<String, Vec<String>> = HashMap::new();
    for (path, _) in &acc.assets {
        let base = path.rsplit('/').next().unwrap_or(path).to_string();
        basename_index.entry(base).or_default().push(path.clone());
    }

    let mut referenced: HashSet<String> = HashSet::new();
    for doc in &acc.docs {
        let path = root.join(doc);
        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() || meta.len() > MAX_DOC_BYTES {
            continue;
        }
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for href in extract_hrefs(&text) {
            if let Some(resolved) = resolve_href(doc, &href) {
                if asset_set.contains(&resolved) {
                    referenced.insert(resolved);
                }
            }
            // Unique basename mention (paste-/excalidraw- stamped names).
            let base = href.rsplit('/').next().unwrap_or(&href).to_string();
            if let Some(paths) = basename_index.get(&base) {
                if paths.len() == 1 {
                    referenced.insert(paths[0].clone());
                }
            }
        }
        // Full root-relative path written in the doc body.
        for (asset_path, _) in &acc.assets {
            if text.contains(asset_path) {
                referenced.insert(asset_path.clone());
            }
        }
    }

    let mut orphans = Vec::new();
    for (path, meta) in acc.assets {
        if referenced.contains(&path) {
            continue;
        }
        orphans.push(meta_to_info(path, &meta, false));
    }
    orphans.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(OrphanAssetsResult {
        orphans,
        asset_count,
        doc_count,
        truncated: acc.truncated,
    })
}
