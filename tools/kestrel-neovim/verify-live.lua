-- Headless Neovim, actual game module, original on-disk sources never written.
local root = vim.fn.getcwd():gsub('\\', '/')
local cache = root .. '/.rules_rust_analyzer/cargo-ide'
local started = vim.uv.hrtime()
local result = { cache_state = 'existing Cargo artifacts; fresh rust-analyzer process', cycles = {}, status = {}, startup_stages = {} }
local function seconds(since) return (vim.uv.hrtime() - (since or started)) / 1e9 end
vim.lsp.log._set_filename(cache .. '/live-lsp.log')
vim.lsp.set_log_level('info')
local config = dofile(root .. '/tools/kestrel-neovim/lsp.lua')(root)
config.settings['rust-analyzer'].checkOnSave = false
config.capabilities.experimental = { serverStatusNotification = true }
local stage_start = {}
config.handlers['$/progress'] = function(_, params)
  if params.value.kind == 'begin' then stage_start[params.token] = seconds() end
  if params.value.kind == 'end' and stage_start[params.token] then
    local ending = seconds()
    result.startup_stages[#result.startup_stages+1] = {
      token = params.token, start_s = stage_start[params.token], end_s = ending,
      seconds = ending - stage_start[params.token],
    }
  end
end
config.handlers['experimental/serverStatus'] = function(_, params)
  result.status[#result.status+1] = { seconds = seconds(), data = params }
  if params and params.quiescent and params.health == 'ok' then result.index_ready_s = seconds() end
end
vim.cmd.edit('kestrel/app/src/flow.rs')
vim.api.nvim_win_set_cursor(0, { 100, 0 })
vim.cmd.redraw()
local buf = vim.api.nvim_get_current_buf()
vim.bo[buf].filetype = 'rust'
local original = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
local probe = dofile(root .. '/tools/kestrel-neovim/probe.lua')(original)
vim.api.nvim_buf_set_lines(buf, 0, -1, false, probe.broken)
vim.api.nvim_win_set_cursor(0, {probe.line, 0})
vim.cmd.redraw()
local client_id = vim.lsp.start(config, { bufnr = buf })
local client = assert(vim.lsp.get_client_by_id(client_id))
local function errors()
  local found = {}
  for _, d in ipairs(vim.diagnostic.get(buf)) do
    if d.lnum >= probe.line - 1 and d.lnum <= probe.line and d.severity == vim.diagnostic.severity.ERROR then found[#found+1] = d end
  end
  return found
end
local function edit(lines)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.cmd.redraw()
end
local function run()
  assert(vim.wait(30000, function() return client.initialized end, 20), 'LSP initialization timed out')
  result.initialize_s = seconds()
  -- A real cross-crate definition, independent of the native-error predicate.
  local col = probe.import_col
  assert(vim.wait(240000, function()
    local response = client:request_sync('textDocument/definition', {
      textDocument = { uri = vim.uri_from_bufnr(buf) }, position = { line = probe.import_line, character = col }
    }, 5000, buf)
    local locations = response and response.result
    if locations and locations[1] then
      result.definition = locations[1]
      result.first_definition_s = seconds()
      return true
    end
  end, 200), 'Cross-crate definition did not resolve')
  assert((result.definition.targetUri or result.definition.uri):find('/kestrel/service/src/lib.rs', 1, true), vim.inspect(result.definition))
  print('First cross-crate definition: ' .. result.first_definition_s .. 's')
  assert(vim.wait(240000, function() return result.index_ready_s ~= nil end, 100), 'Indexing never became quiescent')
  print('Indexing quiescent: ' .. result.index_ready_s .. 's')
  local status = client:request_sync('rust-analyzer/analyzerStatus', {
    textDocument = { uri = vim.uri_from_bufnr(buf) } }, 20000, buf)
  if status and status.result then vim.fn.writefile(vim.split(status.result, '\n'), cache .. '/cargo-analyzer-status.txt') end
  result.direct_diagnostics = client:request_sync('textDocument/diagnostic', {
    textDocument = { uri = vim.uri_from_bufnr(buf) } }, 20000, buf)
  assert(vim.wait(20000, function() return #errors() > 0 end, 20), 'No native diagnostic for actual saved error')
  result.initial_errors = errors()
  local clean = probe.clean
  for _ = 1, 3 do
    local cycle = {}
    local t = vim.uv.hrtime(); edit(clean)
    assert(vim.wait(20000, function() return #errors() == 0 end, 10), 'Native errors did not clear without saving')
    cycle.clear_s = seconds(t)
    t = vim.uv.hrtime(); edit(probe.broken)
    assert(vim.wait(20000, function() return #errors() > 0 end, 10), 'Native errors did not appear without saving')
    cycle.appear_s = seconds(t)
    result.cycles[#result.cycles+1] = cycle
    print('Native cycle: ' .. vim.json.encode(cycle))
  end
  edit(clean)
  assert(vim.wait(20000, function() return #errors() == 0 end, 10), 'Clean buffer did not settle')
  local macro_line, macro_col
  for i, line in ipairs(clean) do
    if line:find('#[derive(Resource)]', 1, true) then
      macro_line = i - 1; macro_col = line:find('Resource', 1, true) - 1; break
    end
  end
  assert(macro_line, 'No Bevy Resource derive in actual module')
  local macro = client:request_sync('rust-analyzer/expandMacro', {
    textDocument = { uri = vim.uri_from_bufnr(buf) }, position = { line = macro_line, character = macro_col }
  }, 20000, buf)
  result.macro = macro
  assert(macro and macro.result and macro.result.expansion:find('impl', 1, true), 'Bevy Resource macro did not expand')
  local semantic = probe.semantic
  local t = vim.uv.hrtime(); edit(semantic)
  local function semantic_error()
    for _, d in ipairs(errors()) do
      if d.code == 'expected-function' or d.code == 'E0618' or d.message:lower():find('expected function') then return true end
    end
  end
  result.semantic_default_enabled = vim.wait(2000, semantic_error, 10)
  if not result.semantic_default_enabled then
    edit(clean)
    assert(vim.wait(20000, function() return #errors() == 0 end, 10), 'Clean buffer did not settle before experimental check')
    config.settings['rust-analyzer'].diagnostics = { experimental = { enable = true } }
    client:notify('workspace/didChangeConfiguration', { settings = config.settings })
    result.semantic_requires_experimental = true
    t = vim.uv.hrtime(); edit(semantic)
  end
  assert(vim.wait(20000, semantic_error, 10), 'Native expected-function semantic diagnostic did not appear: ' .. vim.inspect(errors()))
  result.semantic_appear_s = seconds(t)
  result.semantic_errors = errors()
  t = vim.uv.hrtime(); edit(clean)
  assert(vim.wait(20000, function() return #errors() == 0 end, 10), 'Native semantic diagnostic did not clear')
  result.semantic_clear_s = seconds(t)
  result.pass = true
end
local ok, err = pcall(run)
result.error = not ok and tostring(err) or nil
vim.fn.writefile({vim.json.encode(result)}, cache .. '/live-results.json')
vim.api.nvim_buf_set_lines(buf, 0, -1, false, original)
vim.bo[buf].modified = false
client:stop(true)
print(vim.json.encode({ pass = result.pass, error = result.error, cycles = result.cycles,
  indexing_s = result.index_ready_s, first_definition_s = result.first_definition_s,
  semantic_appear_s = result.semantic_appear_s, semantic_clear_s = result.semantic_clear_s,
  semantic_requires_experimental = result.semantic_requires_experimental, results = cache .. '/live-results.json' }))
if ok then vim.cmd('qa!') else vim.cmd('cquit!') end
