-- Run with the user's normal Neovim config, after enabling this session.
vim.defer_fn(function()
  local ok, err = pcall(function()
    local buf = vim.api.nvim_get_current_buf()
    assert(vim.g.kestrel_cargo_active, 'Session opt-in did not execute')
    assert(vim.wait(30000, function()
      local clients = vim.lsp.get_clients({ bufnr = buf, name = 'kestrel_cargo' })
      return #clients == 1 and clients[1].initialized
    end, 20), 'Opt-in client did not attach with the full Neovim configuration')
    assert(#vim.lsp.get_clients({ bufnr = buf, name = 'rust_analyzer' }) == 0, 'Bazel client remained attached to Kestrel')
    local root = vim.fn.getcwd():gsub('\\', '/')
    local other = vim.fn.bufadd(root .. '/bazel/cargo_manifest_lib.rs')
    local other_root
    vim.lsp.config.rust_analyzer.root_dir(other, function(path) other_root = path end)
    assert(other_root, 'Original Rust configuration no longer accepts files outside kestrel/')
    local client = vim.lsp.get_clients({ bufnr = buf, name = 'kestrel_cargo' })[1]
    assert(client.config.settings['rust-analyzer'].linkedProjects[1]:find('/cargo-ide/Cargo.toml', 1, true))
    local key = vim.fn.maparg('<M-g>', 'n', false, true)
    local result = { pass = true, client = client.name, other_project_root = other_root,
      definition_key = { lhs = key.lhs, rhs = key.rhs, desc = key.desc, callback = type(key.callback) },
      root = client.config.root_dir }
    vim.fn.writefile({ vim.json.encode(result) }, root .. '/.rules_rust_analyzer/cargo-ide/session-results.json')
    print(vim.json.encode(result))
    vim.cmd('KestrelCargoOff')
    assert(not vim.g.kestrel_cargo_active, 'Session opt-out failed')
    local restored
    vim.lsp.config.rust_analyzer.root_dir(buf, function(path) restored = path end)
    assert(restored, 'Original Rust root detection was not restored')
  end)
  for _, client in ipairs(vim.lsp.get_clients()) do client:stop(true) end
  if ok then vim.cmd('qa!') else print(tostring(err)); vim.cmd('cquit!') end
end, 2000)
