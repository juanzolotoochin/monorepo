-- Reproducible probes in the real flow module; never require a broken disk file.
return function(original)
  local clean = vim.deepcopy(original)
  local line
  for i, text in ipairs(clean) do
    if text:find('self.opened.take()', 1, true) then
      line = i
      clean[i] = text:gsub('let __kestrel_syntax_probe = ; ', '')
      break
    end
  end
  assert(line, 'Cannot locate Flow::take_opened diagnostic probe')
  local broken = vim.deepcopy(clean)
  broken[line] = '        let __kestrel_syntax_probe = ; ' .. clean[line]:gsub('^%s*', '')
  local semantic = vim.deepcopy(clean)
  semantic[line] = '        let __kestrel_semantic_probe = 0; __kestrel_semantic_probe(); ' .. clean[line]:gsub('^%s*', '')
  local import_line, import_col
  for i, text in ipairs(clean) do
    local col = text:find('AppServices', 1, true)
    if col then import_line = i - 1; import_col = col - 1; break end
  end
  assert(import_line, 'Cannot locate cross-crate definition probe')
  return { clean = clean, broken = broken, semantic = semantic, line = line,
    import_line = import_line, import_col = import_col }
end
