# 上游同步指南（UPSTREAM SYNC）

本仓库是 LovelyCarve 的本地定制版：在开源上游
（`origin` = https://github.com/LovelyTeams/LovelyCarve）之上维护功能分支。
所有定制集中在 `feat/app-file-tree` 分支，**不要直接改 main**。

## 分支模型

```
origin/main ──► 上游最新（只读，不提交）
      │
      └─ feat/app-file-tree ──► 本地定制（应用聚合树 + 文档/脚本）
```

## 定制内容清单（合并冲突时对照用）

| 改动 | 文件 | 性质 |
|---|---|---|
| 应用聚合树核心 | `src-tauri/src/core/filesystem/app_tree.rs`（新文件） | 纯新增，零冲突 |
| 「应用」拦截 | `src-tauri/src/commands/filesystem/filesystem_apps.rs`（新文件） | 纯新增，零冲突 |
| 拦截 hook | `commands/filesystem/filesystem.rs`、`filesystem_flat.rs` | 每个 hook 3–6 行插入 |
| 缓存字段 | `src-tauri/src/state.rs`（app_indexes 两行） | 追加字段 |
| 模块声明 | `core/filesystem.rs`、`commands/filesystem.rs` | 各 1–2 行 |
| 卸载清缓存 | `app/workflows/evidence/unload.rs` | 1 行 |
| folder 辅助 | `core/filesystem/volumes/folder.rs`（entry_to_node pub(crate)） | 1 词 |
| 前端豁免/图标 | `model/model.ts`、`model/fileTree.ts`、`composables/useFileTreeMetadata.ts`、`useDirectoryMetadata.ts`、`components/display/FileNodeIcon.vue` | 均为追加过滤条件/分支 |

上游若重构了 `fs_children` / `fs_children_page` / `fs_flat_page` 的返回路径，
需要把 hook 重新贴到新路径上（见各文件 `app_tree` / `filesystem_apps` 注释）。

## 同步步骤

1. 运行 `scripts/sync-upstream.ps1`（自动：代理 fetch → merge origin/main →
   cargo test + bun typecheck），或手动：

   ```powershell
   git -c http.proxy=http://127.0.0.1:7890 fetch origin
   git merge origin/main          # 在 feat/app-file-tree 上
   ```

2. 解决冲突（预期集中在 `fs_children` 的返回点与 `state.rs` 字段区）。
3. 验证：`cd src-tauri && cargo test --lib app_tree`（真实检材回归另跑
   `cargo test --lib real_fixture -- --include-ignored`）；
   根目录 `bun run typecheck && bunx vitest run src/features/files`。
4. 构建前端产物后重新编译：`bun run build`（Tauri release 嵌入 dist 资源，
   顺序不能反）→ `cargo build --release`。

## 本地不入库的内容

- `plugins/` 是指向 `..\plugins-repo` 的 NTFS junction（插件 SDK/工作区子模块，
  浅克隆无法在线初始化）。5 个已装插件包复制在 `plugins-repo/<插件 id>/`
  供 debug 构建扫描加载，已用各仓库 `.git/info/exclude` 本地排除，不提交。
- 插件正式安装位置：`%APPDATA%\app.lovelycarve\plugins\`（源码构建与安装版
  共用，identifier 同为 `app.lovelycarve`）。

## 构建环境备忘

- Rust ≥ 1.99（rust-version 钉住；`rustup update stable`）。
- OpenSSL 由 `libsqlite3-sys` vendored 编译，需要 Strawberry Perl
  （`C:\Strawberry`，winget 安装）；Git-Bash 自带 perl 缺模块会失败。
  编译时把 `C:\Strawberry\perl\bin` 放进 PATH。
- GitHub 访问走 Clash 代理 `http://127.0.0.1:7890`。
- **手动 cargo 构建桌面 exe 必须带 custom-protocol**（`tauri build` 会隐式启用，
  裸 cargo 不会——缺了它 WebView 会去连 devUrl，界面报
  `ERR_CONNECTION_REFUSED`）：
  ```powershell
  bun run build   # 先出 dist，release 资源编译期内嵌，顺序不能反
  cargo build --release --features desktop,tauri/custom-protocol
  ```
- **插件加载器必须复制到 exe 旁边**：插件以进程外 `lc-plugin-host` 运行，
  宿主在 `current_exe()` 旁查找（`connection.rs`），cargo build 不会产出——
  缺了所有插件分析报「插件实例启动失败: plugin runtime unavailable」：
  ```powershell
  cargo build --release -p lc-plugin-host   # 在 plugins 工作区
  copy targetelease\lc-plugin-host.exe <构建输出目录>  ```
