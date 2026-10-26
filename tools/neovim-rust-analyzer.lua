-- Install as lua/user/lsp/settings/rust_analyzer.lua in your Neovim config.
-- tools/setup-rust-analyzer.ps1 generates the machine-specific Bazel paths.
local defaults = vim.lsp.config.rust_analyzer

return {
	handlers = {
		-- Neovim 0.11 advertises pull diagnostics but lacks this refresh
		-- handler. Refresh attached buffers when indexing/checking finishes.
		["workspace/diagnostic/refresh"] = vim.lsp.handlers["workspace/diagnostic/refresh"] or function(_, _, ctx)
			vim.schedule(function()
				local client = vim.lsp.get_client_by_id(ctx.client_id)
				if client then
					for bufnr in pairs(client.attached_buffers) do
						vim.lsp.util._refresh("textDocument/diagnostic", { bufnr = bufnr, client_id = client.id })
					end
				end
			end)
			return vim.NIL
		end,
	},
	root_dir = function(bufnr, on_dir)
		local root = vim.fs.root(bufnr, { ".rules_rust_analyzer" })
		if root then
			on_dir(root)
		else
			defaults.root_dir(bufnr, on_dir)
		end
	end,
	cmd = function(dispatchers, config)
		local path = config.root_dir and (config.root_dir .. "/.rules_rust_analyzer/editor-settings.json")
		local cmd = { "rust-analyzer" }
		if path and vim.fn.filereadable(path) == 1 then
			local settings = vim.json.decode(table.concat(vim.fn.readfile(path), "\n"))
			local executable = assert(settings["rust-analyzer.server.path"], "Missing Bazel rust-analyzer executable")
			cmd = { executable }
			-- Mutate the existing settings table: the LSP client already holds
			-- a reference to it when this command factory runs.
			for key, value in pairs(settings) do
				if key == "rust-analyzer.server.extraEnv" then
					config.cmd_env = vim.tbl_extend("force", config.cmd_env or {}, value)
				elseif key ~= "rust-analyzer.server.path" then
					local parts = vim.split(key, ".", { plain = true })
					local target = config.settings
					for i = 1, #parts - 1 do
						target[parts[i]] = target[parts[i]] or {}
						target = target[parts[i]]
					end
					target[parts[#parts]] = value
				end
			end
			local rust = config.settings["rust-analyzer"]
			rust.check = rust.check or {}
			rust.check.overrideCommand = {
				config.root_dir .. "/.rules_rust_analyzer/flycheck.exe",
				"--saved-file",
				"$saved_file",
			}
		end
		config.cmd = cmd
		return vim.lsp.rpc.start(cmd, dispatchers, {
			cwd = config.cmd_cwd or config.root_dir,
			env = config.cmd_env,
			detached = config.detached,
		})
	end,
}
