use std::path::Path;

use git2::{
    AutotagOption, Cred, FetchOptions, IndexAddOption, PushOptions, RemoteCallbacks, Repository,
    Signature,
};
use serde::Serialize;
use tauri::State;

use crate::settings::{self, SettingsState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub dirty_count: usize,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
    pub has_remote: bool,
    pub remote_url: Option<String>,
    pub message: String,
}

fn open_repo(root: &Path) -> Result<Repository, String> {
    Repository::open(root).map_err(|e| format!("打开 git 仓库失败: {e}"))
}

fn ensure_repo(root: &Path) -> Result<Repository, String> {
    match Repository::open(root) {
        Ok(repo) => Ok(repo),
        Err(_) => Err("文档根目录还不是 git 仓库，请先初始化".to_string()),
    }
}

fn make_signature() -> Result<Signature<'static>, String> {
    Signature::now("noteme", "noteme@local").map_err(|e| format!("创建签名失败: {e}"))
}

fn remote_callbacks(token: &str) -> RemoteCallbacks<'_> {
    let mut cb = RemoteCallbacks::new();
    let token = token.to_string();
    cb.credentials(move |_url, _username_from_url, _allowed| {
        Cred::userpass_plaintext("x-access-token", &token)
    });
    cb
}

fn set_origin(repo: &Repository, url: &str) -> Result<(), String> {
    match repo.find_remote("origin") {
        Ok(_) => {
            repo.remote_set_url("origin", url)
                .map_err(|e| format!("更新 origin 失败: {e}"))?;
        }
        Err(_) => {
            repo.remote("origin", url)
                .map_err(|e| format!("添加 origin 失败: {e}"))?;
        }
    }
    Ok(())
}

fn count_dirty(repo: &Repository) -> Result<usize, String> {
    let statuses = repo
        .statuses(None)
        .map_err(|e| format!("读取状态失败: {e}"))?;
    Ok(statuses
        .iter()
        .filter(|e| e.status() != git2::Status::CURRENT)
        .count())
}

fn ahead_behind(repo: &Repository) -> (Option<usize>, Option<usize>) {
    let Ok(head) = repo.head() else {
        return (None, None);
    };
    let Ok(local) = head.peel_to_commit() else {
        return (None, None);
    };
    let branch = head.shorthand().unwrap_or("main");
    let upstream_name = format!("refs/remotes/origin/{branch}");
    let Ok(upstream) = repo.find_reference(&upstream_name) else {
        return (None, None);
    };
    let Ok(remote_commit) = upstream.peel_to_commit() else {
        return (None, None);
    };
    match repo.graph_ahead_behind(local.id(), remote_commit.id()) {
        Ok((a, b)) => (Some(a), Some(b)),
        Err(_) => (None, None),
    }
}

#[tauri::command]
pub fn git_status(state: State<'_, SettingsState>) -> Result<GitStatus, String> {
    let settings = settings::get_settings(&state)?;
    let root = match settings::document_root(&state) {
        Ok(r) => r,
        Err(e) => {
            return Ok(GitStatus {
                is_repo: false,
                branch: None,
                dirty_count: 0,
                ahead: None,
                behind: None,
                has_remote: false,
                remote_url: None,
                message: e,
            });
        }
    };

    let repo = match Repository::open(&root) {
        Ok(r) => r,
        Err(_) => {
            return Ok(GitStatus {
                is_repo: false,
                branch: None,
                dirty_count: 0,
                ahead: None,
                behind: None,
                has_remote: !settings.git.remote_url.trim().is_empty(),
                remote_url: Some(settings.git.remote_url.clone()).filter(|s| !s.trim().is_empty()),
                message: "尚未初始化 git 仓库".to_string(),
            });
        }
    };

    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));
    let dirty_count = count_dirty(&repo)?;
    let (ahead, behind) = ahead_behind(&repo);
    let remote_url = repo
        .find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(|u| u.to_string()))
        .or_else(|| {
            let u = settings.git.remote_url.trim();
            if u.is_empty() {
                None
            } else {
                Some(u.to_string())
            }
        });
    let has_remote = remote_url.is_some();
    let message = if dirty_count > 0 {
        format!("{dirty_count} 个文件有未提交变更")
    } else {
        "工作区干净".to_string()
    };

    Ok(GitStatus {
        is_repo: true,
        branch,
        dirty_count,
        ahead,
        behind,
        has_remote,
        remote_url,
        message,
    })
}

#[tauri::command]
pub fn git_init(state: State<'_, SettingsState>) -> Result<GitStatus, String> {
    let root = settings::document_root(&state)?;
    if Repository::open(&root).is_ok() {
        return Err("已经是 git 仓库".to_string());
    }
    Repository::init(&root).map_err(|e| format!("初始化失败: {e}"))?;
    let settings = settings::get_settings(&state)?;
    if !settings.git.remote_url.trim().is_empty() {
        let repo = open_repo(&root)?;
        set_origin(&repo, settings.git.remote_url.trim())?;
    }
    git_status(state)
}

#[tauri::command]
pub fn git_set_remote(
    state: State<'_, SettingsState>,
    remote_url: String,
) -> Result<(), String> {
    let root = settings::document_root(&state)?;
    let repo = ensure_repo(&root)?;
    let url = remote_url.trim();
    if url.is_empty() {
        return Err("remote URL 不能为空".to_string());
    }
    set_origin(&repo, url)?;
    let mut settings = settings::get_settings(&state)?;
    settings.git.remote_url = url.to_string();
    {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *guard = settings;
    }
    settings::persist(&state)?;
    Ok(())
}

#[tauri::command]
pub fn git_commit(
    state: State<'_, SettingsState>,
    message: Option<String>,
) -> Result<String, String> {
    let root = settings::document_root(&state)?;
    let repo = ensure_repo(&root)?;
    let mut index = repo
        .index()
        .map_err(|e| format!("打开 index 失败: {e}"))?;
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .map_err(|e| format!("暂存失败: {e}"))?;
    index.write().map_err(|e| format!("写入 index 失败: {e}"))?;
    let tree_id = index
        .write_tree()
        .map_err(|e| format!("写入 tree 失败: {e}"))?;
    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| format!("查找 tree 失败: {e}"))?;
    let sig = make_signature()?;
    let msg = message
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "noteme sync {}",
                chrono_like_now()
            )
        });

    let parent_commit = match repo.head() {
        Ok(head) => Some(
            head.peel_to_commit()
                .map_err(|e| format!("读取 HEAD 失败: {e}"))?,
        ),
        Err(_) => None,
    };

    let oid = if let Some(parent) = parent_commit.as_ref() {
        if parent.tree_id() == tree_id {
            return Err("没有可提交的变更".to_string());
        }
        repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &[parent])
            .map_err(|e| format!("提交失败: {e}"))?
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &[])
            .map_err(|e| format!("首次提交失败: {e}"))?
    };
    let short = oid.to_string();
    Ok(format!("已提交 {}", &short[..short.len().min(7)]))
}

fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

#[tauri::command]
pub fn git_pull(state: State<'_, SettingsState>) -> Result<String, String> {
    let settings = settings::get_settings(&state)?;
    let token = settings.git.https_token.trim();
    if token.is_empty() {
        return Err("请先在设置中配置 HTTPS Token".to_string());
    }
    let root = settings::document_root(&state)?;
    let repo = ensure_repo(&root)?;

    let remote_url = settings.git.remote_url.trim();
    if !remote_url.is_empty() {
        set_origin(&repo, remote_url)?;
    }

    let mut remote = repo
        .find_remote("origin")
        .map_err(|_| "未配置 origin remote，请先设置远程地址".to_string())?;

    let cbs = remote_callbacks(token);
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(cbs);
    fetch_opts.download_tags(AutotagOption::All);
    remote
        .fetch(&[] as &[&str], Some(&mut fetch_opts), None)
        .map_err(|e| format!("fetch 失败: {e}"))?;

    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "main".to_string());

    let fetch_head = repo
        .find_reference("FETCH_HEAD")
        .map_err(|e| format!("找不到 FETCH_HEAD: {e}"))?;
    let fetch_commit = repo
        .reference_to_annotated_commit(&fetch_head)
        .map_err(|e| format!("解析 FETCH_HEAD 失败: {e}"))?;

    let analysis = repo
        .merge_analysis(&[&fetch_commit])
        .map_err(|e| format!("merge 分析失败: {e}"))?;

    if analysis.0.is_up_to_date() {
        return Ok("已是最新".to_string());
    }

    if analysis.0.is_fast_forward() {
        let refname = format!("refs/heads/{branch}");
        match repo.find_reference(&refname) {
            Ok(mut r) => {
                r.set_target(fetch_commit.id(), "Fast-Forward")
                    .map_err(|e| format!("快进更新引用失败: {e}"))?;
            }
            Err(_) => {
                repo.reference(&refname, fetch_commit.id(), true, "Fast-Forward")
                    .map_err(|e| format!("创建分支引用失败: {e}"))?;
            }
        }
        repo.set_head(&refname)
            .map_err(|e| format!("设置 HEAD 失败: {e}"))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .map_err(|e| format!("checkout 失败: {e}"))?;
        return Ok("已快进拉取".to_string());
    }

    if analysis.0.is_normal() {
        // Prefer failing clearly rather than leaving a half-merged tree for notes.
        return Err("存在分叉，无法自动合并。请用外部 git 工具解决冲突后再试".to_string());
    }

    Err("无法拉取：不支持的合并情况".to_string())
}

#[tauri::command]
pub fn git_push(state: State<'_, SettingsState>) -> Result<String, String> {
    let settings = settings::get_settings(&state)?;
    let token = settings.git.https_token.trim();
    if token.is_empty() {
        return Err("请先在设置中配置 HTTPS Token".to_string());
    }
    let root = settings::document_root(&state)?;
    let repo = ensure_repo(&root)?;

    let remote_url = settings.git.remote_url.trim();
    if !remote_url.is_empty() {
        set_origin(&repo, remote_url)?;
    }

    let mut remote = repo
        .find_remote("origin")
        .map_err(|_| "未配置 origin remote，请先设置远程地址".to_string())?;

    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "main".to_string());
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");

    let cbs = remote_callbacks(token);
    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(cbs);
    remote
        .push(&[refspec.as_str()], Some(&mut push_opts))
        .map_err(|e| format!("推送失败: {e}"))?;
    Ok(format!("已推送 {branch}"))
}
