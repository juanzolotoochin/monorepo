# Run again after changing the Bazel Rust toolchain or cleaning its cache.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $workspace
try {
    # Bazel's cached executables can be copied with the read-only attribute.
    # Clear it on the installed copies so the upstream installer can rerun.
    foreach ($name in @('discover_bazel_rust_project.exe', 'flycheck.exe')) {
        $path = Join-Path $workspace ".rules_rust_analyzer/$name"
        if (Test-Path -LiteralPath $path) {
            (Get-Item -LiteralPath $path).IsReadOnly = $false
        }
    }
    $snippet = & bazel run '@rules_rust//tools/rust_analyzer:setup' -- print
    if ($LASTEXITCODE -ne 0) {
        throw 'Bazel rust-analyzer setup failed.'
    }

    # Parse before replacing a working configuration. The setup tool writes
    # status messages to stderr and the editor configuration to stdout.
    $settings = ($snippet -join "`n") | ConvertFrom-Json

    # Standard-library discovery also invokes Cargo and rustc. Use the Bazel
    # toolchain rather than whichever rustup toolchain happens to be default.
    & bazel build '@rules_rust//rust/toolchain:current_cargo_files' '@rules_rust//rust/toolchain:current_rustc_files'
    if ($LASTEXITCODE -ne 0) {
        throw 'Bazel Rust toolchain setup failed.'
    }
    $toolchainJson = & bazel cquery '@rules_rust//rust/toolchain:current_rust_toolchain' --output=starlark '--starlark:expr=json.encode({"cargo": providers(target)["ToolchainInfo"].cargo.path, "rustc": providers(target)["ToolchainInfo"].rustc.path})'
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not resolve the Bazel Rust toolchain.'
    }
    $toolchain = ($toolchainJson -join "`n") | ConvertFrom-Json
    $extraEnv = @{}
    foreach ($tool in @('cargo', 'rustc')) {
        $file = Get-Item -LiteralPath (Join-Path $workspace $toolchain.$tool)
        $resolved = $file.ResolveLinkTarget($true)
        $extraEnv[$tool.ToUpperInvariant()] = if ($resolved) { $resolved.FullName } else { $file.FullName }
    }
    # rust-analyzer prefers $CARGO_HOME/bin proxies for some metadata queries.
    # An isolated Cargo home lets the explicit toolchain executables win.
    $extraEnv['CARGO_HOME'] = Join-Path $workspace '.rules_rust_analyzer/cargo-home'
    New-Item -ItemType Directory -Path $extraEnv['CARGO_HOME'] -Force | Out-Null
    $settings | Add-Member -NotePropertyName 'rust-analyzer.server.extraEnv' -NotePropertyValue $extraEnv -Force
    $settings | ConvertTo-Json -Depth 20 |
        Set-Content -LiteralPath '.rules_rust_analyzer/editor-settings.json' -Encoding utf8
    Write-Host 'Rust editor settings saved. Restart the rust_analyzer LSP client in Neovim.'
}
finally {
    Pop-Location
}
