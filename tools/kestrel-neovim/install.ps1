# Install only the personal Rust adapter. Keep the complete previous module.
param([string]$ConfigDirectory = (Join-Path $env:LOCALAPPDATA 'nvim'))
$ErrorActionPreference = 'Stop'
$repoRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)).Replace('\', '/')
& python (Join-Path $PSScriptRoot 'cache.py') status
if ($LASTEXITCODE -ne 0) { throw 'Prepare the Kestrel cache before installing the default.' }
$settingsDirectory = Join-Path $ConfigDirectory 'lua/user/lsp/settings'
$adapter = Join-Path $settingsDirectory 'rust_analyzer.lua'
if (-not (Test-Path -LiteralPath $adapter -PathType Leaf)) {
    throw "Expected the current Rust adapter at $adapter; nothing was installed."
}
$existing = Get-Content -Raw -LiteralPath $adapter
$marker = '-- Kestrel-scoped Cargo default; previous Rust setup is preserved below.'
if ($existing.StartsWith($marker)) {
    $backupLine = ($existing -split "`n" | Where-Object { $_.StartsWith('local backup_path = ') } | Select-Object -First 1)
    if (-not $backupLine) { throw 'Installed shim has no backup reference; refusing to overwrite it.' }
    $backupPath = $backupLine.Substring('local backup_path = '.Length).Trim() | ConvertFrom-Json
    if (-not (Test-Path -LiteralPath $backupPath -PathType Leaf)) { throw 'Original Rust backup is missing.' }
}
else {
    $backupName = 'rust_analyzer.before-kestrel-' + [Guid]::NewGuid().ToString('N').Substring(0, 12) + '.lua'
    $backupPath = Join-Path $settingsDirectory $backupName
    Copy-Item -LiteralPath $adapter -Destination $backupPath
}
$backupPath = (Get-Item -LiteralPath $backupPath).FullName
$luaRoot = $repoRoot | ConvertTo-Json -Compress
$luaBackup = $backupPath.Replace('\', '/') | ConvertTo-Json -Compress
$shim = @"
$marker
local backup_path = $luaBackup
local original = dofile(backup_path)
local root = $luaRoot
local entry = root .. '/tools/kestrel-neovim/default.lua'
if vim.fn.filereadable(entry) ~= 1 then return original end
return dofile(entry)(root, original)
"@
Set-Content -LiteralPath $adapter -Value $shim -Encoding utf8
$record = @{
    adapter = $adapter
    backup = $backupPath
    backup_sha256 = (Get-FileHash -LiteralPath $backupPath).Hash
    installed_sha256 = (Get-FileHash -LiteralPath $adapter).Hash
    scope = "$repoRoot/kestrel/"
    experimental_diagnostics = $false
}
$record | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $repoRoot '.rules_rust_analyzer/cargo-ide/default-install.json') -Encoding utf8
Write-Host "Installed Kestrel-only Cargo default: $adapter"
Write-Host "Preserved original Rust adapter: $backupPath"
Write-Host 'Restart Neovim. Use :KestrelBazel for the session fallback, :KestrelCargo to switch back.'
