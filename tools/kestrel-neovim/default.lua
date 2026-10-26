-- Installed shim calls this factory with the untouched previous Rust config.
-- Register lazily: opening another project never checks Kestrel's cache.
return function(root, original)
  root = root:gsub('\\', '/')
  local original_root = original.root_dir
  local pending = false
  local function activate()
    if vim.g.kestrel_cargo_active then return end
    local ok, err = pcall(dofile, root .. '/tools/kestrel-neovim/enable.lua')
    if not ok then
      vim.notify('Kestrel Cargo setup: ' .. tostring(err)
        .. '\nRun cache.py prepare --all-kestrel, then :KestrelCargo; use :KestrelBazel for the explicit fallback.', vim.log.levels.ERROR)
    end
  end
  vim.api.nvim_create_user_command('KestrelCargo', activate, {
    desc = 'Use the generated Cargo editor model for Kestrel this session', force = true,
  })
  vim.api.nvim_create_user_command('KestrelBazel', function()
    vim.g.kestrel_cargo_disabled = true
    if vim.g.kestrel_cargo_active then
      vim.cmd('KestrelCargoOff')
    else
      vim.lsp.enable('rust_analyzer')
    end
  end, { desc = 'Use the previous Bazel Rust setup for Kestrel this session', force = true })
  original.root_dir = function(buf, on_dir)
    local file = vim.api.nvim_buf_get_name(buf):gsub('\\', '/'):lower()
    local is_game = file:sub(1, #root + 9) == (root .. '/kestrel/'):lower()
    local force_bazel = (vim.env.KESTREL_RUST_MODE or ''):lower() == 'bazel'
    if is_game and not force_bazel and not vim.g.kestrel_cargo_disabled then
      if not pending and not vim.g.kestrel_cargo_active then
        pending = true
        vim.schedule(function()
          if not vim.g.kestrel_cargo_disabled then activate() end
          pending = false
        end)
      end
      return
    end
    return original_root(buf, on_dir)
  end
  return original
end
