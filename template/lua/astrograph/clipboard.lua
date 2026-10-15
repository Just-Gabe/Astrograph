-- Área de transferência sem espera.
--
-- Com `clipboard=unnamedplus`, o Neovim executa um programa externo (win32yank,
-- pbcopy, wl-copy, xclip…) a CADA cópia, esperando ele terminar: `dd` levava
-- ~11 ms (65 ms na primeira vez) contra 0,7 ms sem copiar. No Windows, só criar
-- o processo já custa vários ms. Aqui:
--
--   * copiar (y, d, x, c…) não espera nada: o texto vai ao sistema com atraso
--     agrupado (várias cópias seguidas viram uma) e na hora em que alguém de
--     fora pode precisar dele: ao sair da janela ou fechar o Neovim;
--   * colar usa a cópia em memória enquanto ninguém pode ter copiado nada fora
--     do Neovim (até o terminal perder o foco ou você sair de um terminal de
--     agente). Além de instantâneo, preserva o tipo do registro (linha inteira
--     vs. trecho), que o sistema perde. Só vale em terminais que avisam o foco;
--   * depois de sair e voltar, lê do sistema como sempre.
--
-- Sem um programa conhecido no PATH, mantém o comportamento padrão do Neovim.
-- Desligue com `vim.g.astrograph_clipboard = false` no topo do init.lua.

local M = {}

M.delay = 400 -- ms até mandar a última cópia ao sistema

local providers = {
  { bin = "win32yank.exe", copy = { "win32yank.exe", "-i", "--crlf" }, paste = { "win32yank.exe", "-o", "--lf" } },
  { bin = "win32yank", copy = { "win32yank", "-i", "--crlf" }, paste = { "win32yank", "-o", "--lf" } },
  { bin = "pbcopy", copy = { "pbcopy" }, paste = { "pbpaste" } },
  {
    bin = "wl-copy",
    copy = { "wl-copy", "--type", "text/plain" },
    paste = { "wl-paste", "--no-newline" },
    env = "WAYLAND_DISPLAY",
  },
  {
    bin = "xclip",
    copy = { "xclip", "-quiet", "-i", "-selection", "clipboard" },
    paste = { "xclip", "-o", "-selection", "clipboard" },
    env = "DISPLAY",
  },
  { bin = "xsel", copy = { "xsel", "--nodetach", "-i", "-b" }, paste = { "xsel", "-o", "-b" }, env = "DISPLAY" },
}

-- só os programas do sistema atual (no Windows, checar um binário ausente
-- custa ~20 ms)
local function candidates()
  if vim.fn.has "win32" == 1 then return { providers[1], providers[2] } end
  if vim.fn.has "mac" == 1 then return { providers[3] } end
  return { providers[4], providers[5], providers[6] }
end

local function pick()
  -- o Neovim para Windows traz o win32yank na própria pasta: olhar ali custa
  -- ~0,4 ms, contra ~5 ms procurando no PATH
  if vim.fn.has "win32" == 1 then
    local bundled = vim.fs.joinpath(vim.fs.dirname(vim.v.progpath), "win32yank.exe")
    if (vim.uv or vim.loop).fs_stat(bundled) then
      return {
        bin = "win32yank.exe",
        copy = { bundled, "-i", "--crlf" },
        paste = { bundled, "-o", "--lf" },
      }
    end
  end
  for _, p in ipairs(candidates()) do
    if (not p.env or vim.env[p.env]) and vim.fn.executable(p.bin) == 1 then return p end
  end
end

-- cópia em memória por registro ("+" e "*")
local cache = {} ---@type table<string, {lines: string[], regtype: string}?>

-- só confiamos na cópia em memória depois de ver um evento de foco: em
-- terminais que não os enviam, ela nunca seria invalidada
local focus_events = false

function M.setup()
  if vim.g.astrograph_clipboard == false or vim.g.clipboard ~= nil then return end
  local p = pick()
  if not p then return end

  local pending ---@type string?
  local timer = vim.uv.new_timer()

  ---Manda ao sistema o que ainda não foi.
  ---@param wait? boolean espera o programa terminar (ao fechar o Neovim)
  local function flush(wait)
    timer:stop()
    if not pending then return end
    local text = pending
    pending = nil
    local ok, job = pcall(vim.system, p.copy, { stdin = text })
    if ok and wait then pcall(job.wait, job, 1000) end
  end
  M.flush = flush

  local function copy(reg)
    return function(lines, regtype)
      lines = vim.deepcopy(lines)
      -- o Neovim manda a linha vazia final nas cópias de linha inteira
      if regtype == "V" and #lines > 1 and lines[#lines] == "" then table.remove(lines) end
      cache[reg] = { lines = lines, regtype = regtype }
      pending = table.concat(lines, "\n") .. (regtype == "V" and "\n" or "")
      timer:stop()
      timer:start(M.delay, 0, vim.schedule_wrap(function() flush(false) end))
    end
  end

  local function paste(reg)
    return function()
      local c = focus_events and cache[reg] or nil
      if c then return { c.lines, c.regtype } end
      flush(true) -- uma cópia nossa ainda pendente precisa chegar antes de ler
      local ok, res = pcall(function() return vim.system(p.paste, { text = true }):wait(2000) end)
      if not ok or not res or res.code ~= 0 then return { {}, "v" } end
      local text = (res.stdout or ""):gsub("\r\n", "\n")
      local linewise = text:sub(-1) == "\n"
      if linewise then text = text:sub(1, -2) end
      return { vim.split(text, "\n", { plain = true }), linewise and "V" or "v" }
    end
  end

  vim.g.clipboard = {
    name = "astrograph-" .. p.bin,
    copy = { ["+"] = copy "+", ["*"] = copy "*" },
    paste = { ["+"] = paste "+", ["*"] = paste "*" },
    cache_enabled = 0,
  }

  -- fora do Neovim (ou num terminal de agente dentro dele) alguém pode copiar
  -- outra coisa: o que foi copiado aqui vai ao sistema, e a próxima colagem lê de lá
  local group = vim.api.nvim_create_augroup("astrograph_clipboard", { clear = true })
  vim.api.nvim_create_autocmd({ "FocusLost", "TermLeave" }, {
    group = group,
    desc = "Mandar a cópia ao sistema e invalidar a cópia em memória",
    callback = function(ev)
      if ev.event == "FocusLost" then focus_events = true end
      flush(false)
      cache = {}
    end,
  })
  vim.api.nvim_create_autocmd("FocusGained", {
    group = group,
    desc = "O terminal envia eventos de foco",
    callback = function() focus_events = true end,
  })
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    desc = "Garantir a última cópia na área de transferência ao sair",
    callback = function() flush(true) end,
  })
end

return M
