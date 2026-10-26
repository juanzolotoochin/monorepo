-- Kestrel activation shared by the default router and explicit session launcher.
-- Other projects and Rust clients keep their original configuration.
local script = vim.fn.fnamemodify(debug.getinfo(1, 'S').source:sub(2), ':p')
local root = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(script))):gsub('\\', '/')
local cache = root .. '/.rules_rust_analyzer/cargo-ide'
if vim.g.kestrel_cargo_active then return end
local make_config = dofile(root .. '/tools/kestrel-neovim/lsp.lua')
local config = make_config(root)
vim.g.kestrel_cargo_disabled = false
local function is_kestrel(buf)
  local file = vim.api.nvim_buf_get_name(buf):gsub('\\', '/'):lower()
  return file:sub(1, #root + 9) == (root .. '/kestrel/'):lower()
end
local previous = vim.deepcopy(vim.lsp.config.rust_analyzer)
local previous_root = previous and previous.root_dir
local previously_enabled = vim.lsp.is_enabled('rust_analyzer')
if previous then
  config.on_attach = previous.on_attach
  config.capabilities = vim.tbl_deep_extend('force', previous.capabilities or {}, config.capabilities)
  vim.lsp.config('rust_analyzer', {
    root_dir = function(buf, on_dir)
      if is_kestrel(buf) then return end
      if type(previous_root) == 'function' then
        previous_root(buf, on_dir)
      elseif type(previous_root) == 'string' then
        on_dir(previous_root)
      else
        local found = vim.fs.root(buf, { 'Cargo.toml', 'rust-project.json', '.git' })
        if found then on_dir(found) end
      end
    end,
  })
end
config.root_dir = function(buf, on_dir)
  if is_kestrel(buf) then on_dir(cache) end
end
vim.lsp.config('kestrel_cargo', config)
vim.g.kestrel_cargo_active = true
local group = vim.api.nvim_create_augroup('KestrelCargoOptIn', { clear = true })
local function detach_bazel(buf, client)
  vim.lsp.buf_detach_client(buf, client.id)
  if not next(client.attached_buffers) then client:stop() end
end
-- A default client may already be initializing when VimEnter enables the
-- opt-in. Guard its eventual attachment as well as clients attached now.
vim.api.nvim_create_autocmd('LspAttach', {
  group = group,
  callback = function(event)
    local client = vim.lsp.get_client_by_id(event.data.client_id)
    if vim.g.kestrel_cargo_active and is_kestrel(event.buf) and client and client.name == 'rust_analyzer' then
      vim.schedule(function()
        -- Neovim queues semantic-token setup after LspAttach. Let that
        -- callback finish before detaching, so it cannot restart a stale client.
        vim.schedule(function()
          if vim.g.kestrel_cargo_active and client.attached_buffers[event.buf] then detach_bazel(event.buf, client) end
        end)
      end)
    end
  end,
})
for _, buf in ipairs(vim.api.nvim_list_bufs()) do
  if vim.api.nvim_buf_is_loaded(buf) and is_kestrel(buf) then
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = buf, name = 'rust_analyzer' })) do
      detach_bazel(buf, client)
    end
  end
end
vim.lsp.enable('kestrel_cargo')
if vim.v.vim_did_enter == 0 then
  vim.api.nvim_create_autocmd('VimEnter', {
    group = group, once = true,
    callback = function()
      if vim.g.kestrel_cargo_active then vim.lsp.enable('kestrel_cargo') end
    end,
  })
end
vim.api.nvim_create_user_command('KestrelCargoOff', function()
  vim.g.kestrel_cargo_disabled = true
  -- Detach immediately; graceful process shutdown can otherwise leave stale
  -- Cargo diagnostics attached while the fallback client is already starting.
  for _, client in ipairs(vim.lsp.get_clients({ name = 'kestrel_cargo' })) do
    for _, buf in ipairs(vim.tbl_keys(client.attached_buffers)) do
      vim.lsp.buf_detach_client(buf, client.id)
    end
    client:stop()
  end
  vim.lsp.enable('kestrel_cargo', false)
  if previous then vim.lsp.config.rust_analyzer = previous end
  vim.g.kestrel_cargo_active = false
  vim.api.nvim_del_augroup_by_id(group)
  if previously_enabled then vim.lsp.enable('rust_analyzer') end
end, { desc = 'Restore the previous Rust setup for this Neovim session', force = true })
vim.notify('Kestrel Cargo editor cache enabled for kestrel/ in this session', vim.log.levels.INFO)
