# 文档根目录与本地 Git 同步

noteme 直接读写操作系统上的文档根目录，并通过 [git2](https://docs.rs/git2)（libgit2）与 GitHub 等远程仓库做手动同步。

## 配置（设置页）

设置持久化在应用数据目录的 `settings.json`：

```json
{
  "documentRoot": "/Users/me/Notes",
  "git": {
    "remoteUrl": "https://github.com/user/notes.git",
    "httpsToken": "ghp_..."
  }
}
```

| 字段 | 说明 |
|------|------|
| `documentRoot` | 笔记与附件所在目录；侧边栏路径均相对此根 |
| `git.remoteUrl` | HTTPS 远程地址（`origin`） |
| `git.httpsToken` | GitHub / GitLab 等 Personal Access Token，仅存本地 |

在「文档目录」中用系统对话框选择根目录（仅保存路径，不扫描全树）；侧边栏按目录懒加载。在「Git 同步」中填写 remote 与 token。

## 同步流程

1. 选择文档根目录（可为空目录或已有笔记目录）。
2. （可选）「初始化仓库」——在根目录执行 `git init`，并写入 `origin`。
3. 编辑笔记后，在「Git 同步」中：**提交** → **推送**；从远程更新用 **拉取**。
4. HTTPS 认证用户名为 `x-access-token`，密码为 PAT。

## 行为说明

- **提交**：暂存全部变更（尊重 `.gitignore`）后 commit；无变更则报错。
- **拉取**：`fetch` 后优先快进；若出现分叉需用外部 git 工具解决冲突后再拉。
- **推送**：推送当前分支到 `origin` 同名分支。
- Token 与远程 URL 不会写入文档仓库本身。

## 明确不做（当前版本）

- SSH 认证
- 保存时自动 commit / push
- 从远程一键 clone 为文档根
- 冲突可视化合并 UI
