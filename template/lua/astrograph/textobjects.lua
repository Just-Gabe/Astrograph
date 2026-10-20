-- Textobjects pelo app Astrograph (Rust), com o nvim-treesitter-textobjects como
-- reserva.
--
-- O app mantém a árvore de cada buffer e responde, por rpcrequest, ao trecho do
-- objeto no cursor (`vaf`, `dif`...) e à lista de trechos de uma captura (o que o
-- mover `]f` e o trocar `>F` filtram). Aqui só trocamos as duas funções do plugin
-- que leem a árvore; seleção, operador, repetição com `.`, saltos e a troca de
-- texto continuam sendo as do plugin. O app marca o buffer com
-- `b:astrograph_textobjects` (canal do app). Sem a marca, sem resposta ou com a
-- cópia atrasada (`false`), vale o plugin. `ASTROGRAPH_TEXTOBJ=nvim` desliga.

local M = {}

---@param bufnr integer
---@return integer?
local function channel(bufnr)
  local ok, ch = pcall(function() return vim.b[bufnr].astrograph_textobjects end)
  if ok and type(ch) == "number" and ch > 0 then return ch end
end

---Pergunta ao app. `ok=false`: usar o plugin.
---@return boolean ok, any result
local function ask(ch, name, ...)
  local ok, res = pcall(vim.rpcrequest, ch, name, ...)
  if not ok or res == false then return false end
  if res == vim.NIL then res = nil end
  return true, res
end

local function at_prefix(capture) return capture:sub(1, 1) == "@" and capture or "@" .. capture end

---Troca as leituras de árvore do plugin (idempotente).
function M.patch()
  local ok, shared = pcall(require, "nvim-treesitter-textobjects.shared")
  if not ok or shared._astrograph then return end
  shared._astrograph = true
  local plugin_at, plugin_best = shared.textobject_at_point, shared.find_best_range

  shared.textobject_at_point = function(query, group, bufnr, pos, opts)
    bufnr = bufnr or vim.api.nvim_get_current_buf()
    local ch = channel(bufnr)
    if ch and (group == nil or group == "textobjects") then
      local p = pos or vim.api.nvim_win_get_cursor(0)
      local o = opts or {}
      local answered, range = ask(
        ch,
        "astrograph_textobject",
        bufnr,
        vim.api.nvim_buf_get_changedtick(bufnr),
        at_prefix(query),
        p[1] - 1,
        p[2],
        o.lookahead and true or false,
        o.lookbehind and true or false
      )
      if answered then return range end
    end
    return plugin_at(query, group, bufnr, pos, opts)
  end

  shared.find_best_range = function(bufnr, capture, group, filter, score)
    local ch = channel(bufnr)
    if ch and (group == nil or group == "textobjects") then
      local answered, ranges =
        ask(ch, "astrograph_textobject_ranges", bufnr, vim.api.nvim_buf_get_changedtick(bufnr), at_prefix(capture))
      if answered and type(ranges) == "table" then
        local best, best_score
        for _, r in ipairs(ranges) do
          if filter(r) then
            local s = score(r)
            if not best or s > best_score then best, best_score = r, s end
          end
        end
        return best
      end
    end
    return plugin_best(bufnr, capture, group, filter, score)
  end
end

function M.setup()
  if vim.env.ASTROGRAPH_TEXTOBJ == "nvim" then return end
  require("astrocore").on_load("nvim-treesitter-textobjects", M.patch)
end

return M
