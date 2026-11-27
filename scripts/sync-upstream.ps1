# 上游同步脚本：fetch origin -> merge origin/main -> 回归验证
# 用法: powershell -ExecutionPolicy Bypass -File scripts\sync-upstream.ps1 [-DryRun]
param(
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot   # 仓库根（scripts/ 的上一级）
$proxy = "http://127.0.0.1:7890"
$branch = "feat/app-file-tree"

Set-Location $repo
$current = git rev-parse --abbrev-ref HEAD
if ($current -ne $branch) {
    Write-Host "当前分支 $current != $branch，先切分支" -ForegroundColor Yellow
    git checkout $branch
}

Write-Host "==> fetch origin (proxy $proxy)" -ForegroundColor Cyan
git -c http.proxy=$proxy -c https.proxy=$proxy fetch origin
$behind = git rev-list --count HEAD..origin/main
if ($behind -eq "0") {
    Write-Host "已与 origin/main 同步，无需合并。" -ForegroundColor Green
    exit 0
}
Write-Host "落后 origin/main $behind 个提交，开始合并..." -ForegroundColor Yellow
if ($DryRun) { Write-Host "(DryRun) 跳过合并与验证"; exit 0 }

git merge origin/main
if ($LASTEXITCODE -ne 0) {
    Write-Host "存在冲突：解决后重跑本脚本以继续验证。" -ForegroundColor Red
    Write-Host "冲突面参考 docs/UPSTREAM-SYNC.md 的定制内容清单。"
    exit 1
}

Write-Host "==> 后端测试 (src-tauri)" -ForegroundColor Cyan
Push-Location "$repo\src-tauri"
$env:PATH = "C:\Strawberry\perl\bin;$env:PATH"
cargo test --lib app_tree
if ($LASTEXITCODE -ne 0) { Pop-Location; exit 1 }
Pop-Location

Write-Host "==> 前端类型检查 + 文件页测试" -ForegroundColor Cyan
bun run typecheck
if ($LASTEXITCODE -ne 0) { exit 1 }
bunx vitest run src/features/files
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "同步完成。重新构建：bun run build && cargo build --release" -ForegroundColor Green
