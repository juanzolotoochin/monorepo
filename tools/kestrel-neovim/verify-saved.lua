-- Saved compiler checks in a faithful copy. No writes to the original kestrel/.
local root = vim.fn.getcwd():gsub('\\', '/')
local cache = root .. '/.rules_rust_analyzer/cargo-ide'
local overlay = cache .. '/validation-overlay'
local started = vim.uv.hrtime()
local result = { overlay = overlay, cycles = {}, progress = {}, checks_finished = 0 }
local function seconds(since) return (vim.uv.hrtime() - (since or started)) / 1e9 end
vim.lsp.log._set_filename(cache .. '/saved-lsp.log')
vim.lsp.set_log_level('info')
local config = dofile(root .. '/tools/kestrel-neovim/lsp.lua')(root)
config.root_dir = overlay .. '/workspace'
config.settings['rust-analyzer'].linkedProjects = { overlay .. '/workspace/Cargo.toml' }
config.capabilities.experimental = { serverStatusNotification = true }
config.handlers['experimental/serverStatus'] = function(_, params)
  if params and params.health == 'ok' and params.quiescent then result.index_ready_s = seconds() end
end
local titles = {}
config.handlers['$/progress'] = function(err, params, ctx)
  if params.value.kind ~= 'report' then
    result.progress[#result.progress+1] = { seconds = seconds(), params = params }
  end
  if params.value.kind == 'begin' then titles[params.token] = params.value.title or '' end
  if params.value.kind == 'end' and
      (tostring(params.token):find('flycheck', 1, true) or (titles[params.token] or ''):lower():find('check', 1, true)) then
    result.checks_finished = result.checks_finished + 1
  end
end
vim.cmd.edit(overlay .. '/source/kestrel/app/src/flow.rs')
vim.api.nvim_win_set_cursor(0, { 100, 0 })
vim.cmd.redraw()
local buf = vim.api.nvim_get_current_buf()
vim.bo[buf].filetype = 'rust'
local original = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
local probe = dofile(root .. '/tools/kestrel-neovim/probe.lua')(original)
local clean = probe.clean
vim.api.nvim_win_set_cursor(0, {probe.line, 0})
vim.cmd.redraw()
local id = vim.lsp.start(config, { bufnr = buf })
local client = assert(vim.lsp.get_client_by_id(id))
local function compiler_errors()
  local found = {}
  for _, d in ipairs(vim.diagnostic.get(buf)) do
    if d.source == 'rustc' and d.severity == vim.diagnostic.severity.ERROR then found[#found+1] = d end
  end
  return found
end
local function save(lines)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.cmd.write()
  vim.cmd.redraw()
end
local function run()
  assert(vim.wait(240000, function()
    return result.index_ready_s and result.checks_finished > 0 and #compiler_errors() > 0
  end, 20), 'Initial compiler diagnostics/check completion missing: ' .. vim.inspect(vim.diagnostic.get(buf)))
  result.first_compiler_and_index_ready_s = seconds()
  result.initial_errors = compiler_errors()
  print('Overlay initial compiler diagnostics and indexing ready: ' .. seconds() .. 's')
  for _ = 1, 3 do
    local cycle = {}
    local previous, t = result.checks_finished, vim.uv.hrtime()
    save(clean)
    assert(vim.wait(120000, function() return result.checks_finished > previous and #compiler_errors() == 0 end, 10), 'Saved compiler errors did not clear')
    cycle.clear_s = seconds(t)
    for _, d in ipairs(vim.diagnostic.get(nil)) do
      assert(d.source ~= 'rustc' or d.severity ~= vim.diagnostic.severity.ERROR,
        'Another workspace compiler error remains after the clean check: ' .. d.message)
    end
    previous, t = result.checks_finished, vim.uv.hrtime()
    save(original)
    assert(vim.wait(120000, function() return result.checks_finished > previous and #compiler_errors() > 0 end, 10), 'Saved compiler errors did not appear')
    cycle.appear_s = seconds(t)
    result.cycles[#result.cycles+1] = cycle
    print('Overlay compiler cycle: ' .. vim.json.encode(cycle))
  end
  result.pass = true
end
local ok, err = pcall(run)
if not ok then result.error = tostring(err) end
vim.fn.writefile({vim.json.encode(result)}, cache .. '/saved-results.json')
client:stop(true)
print(vim.json.encode({ pass = result.pass, error = result.error, cycles = result.cycles,
  initial_ready_s = result.first_compiler_and_index_ready_s, results = cache .. '/saved-results.json' }))
if ok then vim.cmd('qa!') else vim.cmd('cquit!') end
