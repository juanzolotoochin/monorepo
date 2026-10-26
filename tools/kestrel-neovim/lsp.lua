-- Shared Kestrel Cargo configuration; does not install or enable itself.
return function(root)
  local cache = root .. '/.rules_rust_analyzer/cargo-ide'
  local status = vim.system({ 'python', root .. '/tools/kestrel-neovim/cache.py', 'status' }, { text = true }):wait()
  assert(status.code == 0, status.stderr .. status.stdout)
  local generated = vim.json.decode(table.concat(vim.fn.readfile(cache .. '/neovim.json'), '\n'))
  local rust = generated.settings['rust-analyzer']
  rust.diagnostics = rust.diagnostics or {}
  rust.diagnostics.experimental = { enable = false }
  rust.checkOnSave = true
  local capabilities = vim.lsp.protocol.make_client_capabilities()
  capabilities.workspace.diagnostics = { refreshSupport = true }
  return {
    name = 'kestrel_cargo',
    cmd = generated.cmd,
    cmd_env = generated.env,
    cmd_cwd = cache,
    root_dir = cache,
    filetypes = { 'rust' },
    capabilities = capabilities,
    settings = generated.settings,
    init_options = vim.deepcopy(generated.settings['rust-analyzer']),
    before_init = function(params, config)
      params.initializationOptions = vim.deepcopy(config.settings['rust-analyzer'])
    end,
    handlers = {
      ['workspace/diagnostic/refresh'] = function(_, _, ctx)
        vim.schedule(function()
          local client = vim.lsp.get_client_by_id(ctx.client_id)
          if client then
            for bufnr in pairs(client.attached_buffers) do
              vim.lsp.util._refresh('textDocument/diagnostic', { bufnr = bufnr, client_id = client.id })
            end
          end
        end)
        return vim.NIL
      end,
    },
  }
end
