# Convenience Kestrel session launcher; installing a default is optional.
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$EditorArgs)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
& python (Join-Path $PSScriptRoot 'cache.py') status
if ($LASTEXITCODE -ne 0) { throw 'Prepare the Kestrel editor cache first.' }
$previousKestrelRoot = $env:KESTREL_CARGO_IDE_ROOT
try {
    $env:KESTREL_CARGO_IDE_ROOT = $repoRoot.Replace('\', '/')
    $startup = "lua vim.api.nvim_create_autocmd('VimEnter', {once=true, callback=function() dofile(vim.env.KESTREL_CARGO_IDE_ROOT .. '/tools/kestrel-neovim/enable.lua') end})"
    & nvim --cmd $startup @EditorArgs
    $editorExit = $LASTEXITCODE
}
finally {
    $env:KESTREL_CARGO_IDE_ROOT = $previousKestrelRoot
}
exit $editorExit
