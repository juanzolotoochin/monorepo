-- Plain Neovim with the installed personal adapter. No original file writes.
vim.defer_fn(function()
  local root = vim.fn.getcwd():gsub('\\', '/')
  local cache = root .. '/.rules_rust_analyzer/cargo-ide'
  local buf = vim.api.nvim_get_current_buf()
  local original = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  local probe = dofile(root .. '/tools/kestrel-neovim/probe.lua')(original)
  local result = { startup_mode = vim.env.KESTREL_RUST_MODE or 'cargo', native_cycles = {} }
  local function wait_client(name)
    local client
    assert(vim.wait(30000, function()
      local found = vim.lsp.get_clients({ bufnr = buf, name = name })
      client = found[1]
      return #found == 1 and client.initialized
    end, 20), 'Expected one initialized ' .. name .. ' client')
    return client
  end
  local function assert_cargo(client)
    local rust = client.config.settings['rust-analyzer']
    assert(rust.diagnostics.experimental.enable == false, 'Experimental diagnostics must remain disabled')
    assert(rust.checkOnSave == true, 'Saved checks must remain enabled')
    assert(rust.linkedProjects[1] == cache .. '/Cargo.toml', 'Wrong Cargo project')
    assert(#vim.lsp.get_clients({ bufnr = buf, name = 'rust_analyzer' }) == 0, 'Duplicate Bazel client')
  end
  local function native_errors()
    local found = {}
    for _, d in ipairs(vim.diagnostic.get(buf)) do
      if d.source == 'rust-analyzer' and d.severity == vim.diagnostic.severity.ERROR and d.lnum >= probe.line - 1 and d.lnum <= probe.line then
        found[#found+1] = d
      end
    end
    return found
  end
  local function assert_bazel(client)
    assert(client.config.root_dir:gsub('\\', '/'):lower() == root:lower(), 'Wrong fallback root')
    local rust = client.config.settings['rust-analyzer']
    assert(rust.workspace.discoverConfig, 'Original Bazel discovery missing')
    assert(rust.check.overrideCommand[1]:find('flycheck.exe', 1, true), 'Original checker missing')
    assert(#vim.lsp.get_clients({ bufnr = buf, name = 'kestrel_cargo' }) == 0, 'Cargo client remains during fallback')
  end
  local function run()
    if result.startup_mode == 'bazel' then
      assert_bazel(wait_client('rust_analyzer'))
      assert(not vim.g.kestrel_cargo_active, 'Cargo activated despite the startup fallback')
      result.environment_fallback = true
      return
    end
    local client = wait_client('kestrel_cargo')
    assert_cargo(client)
    result.default_cargo = true
    -- Outside-game and standalone Cargo roots must still use the old selector.
    local other = vim.fn.bufadd(root .. '/bazel/cargo_manifest_lib.rs')
    local other_root
    vim.lsp.config.rust_analyzer.root_dir(other, function(path) other_root = path end)
    assert(other_root and other_root:gsub('\\', '/'):lower() == root:lower(), 'Outside-game Bazel root changed')
    result.outside_game_root = other_root
    local temp = vim.fn.tempname()
    vim.fn.mkdir(temp .. '/src', 'p')
    vim.fn.writefile({ '[package]', 'name = "kestrel-routing-check"', 'version = "0.0.0"', 'edition = "2021"' }, temp .. '/Cargo.toml')
    vim.fn.writefile({ 'pub fn routing_check() {}' }, temp .. '/src/lib.rs')
    local standalone = vim.fn.bufadd(temp .. '/src/lib.rs')
    local standalone_root
    vim.lsp.config.rust_analyzer.root_dir(standalone, function(path) standalone_root = path end)
    local standalone_ok = vim.wait(15000, function() return standalone_root ~= nil end, 20)
    vim.fn.delete(temp .. '/src/lib.rs'); vim.fn.delete(temp .. '/Cargo.toml')
    vim.fn.delete(temp .. '/Cargo.lock'); vim.fn.delete(temp .. '/src', 'd'); vim.fn.delete(temp, 'd')
    assert(standalone_ok and standalone_root:gsub('\\', '/'):lower() == temp:gsub('\\', '/'):lower(), 'Standalone Cargo root changed: ' .. tostring(standalone_root))
    result.standalone_cargo_preserved = true
    local key = vim.fn.maparg('<M-g>', 'n', false, true)
    assert(key.rhs and key.rhs:find('vim.lsp.buf.definition', 1, true), 'Alt-G changed')
    result.alt_g = key.rhs
    -- Test stable native diagnostics in the installed client without compiler
    -- diagnostics from the intentional disk error obscuring the buffer checks.
    local rust = client.config.settings['rust-analyzer']
    rust.checkOnSave = false
    client:notify('workspace/didChangeConfiguration', { settings = client.config.settings })
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, probe.broken)
    vim.api.nvim_win_set_cursor(0, {probe.line, 0}); vim.cmd.redraw()
    assert(vim.wait(120000, function() return #native_errors() > 0 end, 20), 'Installed client did not report the actual native syntax errors')
    local symbol_col = probe.import_col
    local definition = client:request_sync('textDocument/definition', {
      textDocument = { uri = vim.uri_from_bufnr(buf) }, position = { line = probe.import_line, character = symbol_col },
    }, 30000, buf)
    local location = definition and definition.result and definition.result[1]
    local uri = location and (location.targetUri or location.uri)
    assert(uri and uri:lower():find('/kestrel/service/src/lib.rs', 1, true), 'Installed cross-crate definition failed')
    result.cross_crate_definition = uri
    local clean = probe.clean
    for _ = 1, 2 do
      local cycle = {}
      local started = vim.uv.hrtime()
      vim.api.nvim_buf_set_lines(buf, 0, -1, false, clean)
      assert(vim.wait(20000, function() return #native_errors() == 0 end, 10), 'Stable native errors did not clear')
      cycle.clear_s = (vim.uv.hrtime() - started) / 1e9
      started = vim.uv.hrtime()
      vim.api.nvim_buf_set_lines(buf, 0, -1, false, probe.broken)
      assert(vim.wait(20000, function() return #native_errors() > 0 end, 10), 'Stable native errors did not reappear')
      cycle.appear_s = (vim.uv.hrtime() - started) / 1e9
      result.native_cycles[#result.native_cycles+1] = cycle
    end
    rust.checkOnSave = true
    client:notify('workspace/didChangeConfiguration', { settings = client.config.settings })
    vim.cmd('KestrelBazel')
    assert_bazel(wait_client('rust_analyzer'))
    result.command_fallback = true
    vim.cmd('KestrelCargo')
    assert_cargo(wait_client('kestrel_cargo'))
    result.command_return_to_cargo = true
  end
  local ok, err = pcall(run)
  result.pass = ok
  if not ok then result.error = tostring(err) end
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, original)
  vim.bo[buf].modified = false
  local suffix = result.startup_mode == 'bazel' and '-bazel' or ''
  vim.fn.writefile({vim.json.encode(result)}, cache .. '/default-results' .. suffix .. '.json')
  local clients = vim.lsp.get_clients()
  for _, client in ipairs(clients) do client:stop() end
  vim.wait(5000, function()
    for _, client in ipairs(clients) do if not client:is_stopped() then return false end end
    return true
  end, 20)
  for _, client in ipairs(clients) do if not client:is_stopped() then client:stop(true) end end
  print(vim.json.encode(result))
  if ok then vim.cmd('qa!') else vim.cmd('cquit!') end
end, 1500)
