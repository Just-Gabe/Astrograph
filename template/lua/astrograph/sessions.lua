-- Sessões do agente (avante) de todos os projetos, com a tag do modelo.
--
--   <Leader>AS / :Astrograph sessoes   lista as conversas de todos os projetos
--                                       (mais recente primeiro) e retoma a escolhida
--
-- O avante guarda cada conversa num JSON por projeto, mas só lista as do projeto
-- atual (:AvanteHistory) e não diz com qual agente/modelo ela foi feita. Aqui:
--   * cada gravação do avante ganha a tag `astrograph = { provider, model }`;
--   * um índice pequeno (astrograph-sessoes.json, na pasta do avante) guarda pasta
--     do projeto, título, tag e nº de mensagens, para listar tudo sem abrir os
--     JSONs, que podem passar de 30 MB (abrir um desses travava o editor);
--   * conversas grandes aparecem com aviso e pedem confirmação antes de abrir.

local M = {}

M.HEAVY = 5 * 1024 * 1024 -- bytes: acima disso o avante trava ao abrir/gravar
local PEEK = 1024 * 1024 -- conversas sem índice e até este tamanho têm o título lido
local INDEX = "astrograph-sessoes.json"

local uv = vim.uv or vim.loop

local function notify(msg, level) vim.notify(msg, level or vim.log.levels.INFO, { title = "Astrograph" }) end

---Pasta onde o avante grava (a mesma do `history.storage_path`).
---@return string
function M.storage()
  local ok, Config = pcall(require, "avante.config")
  local p = ok and Config.history and Config.history.storage_path
  return vim.fs.normalize(p or vim.fs.joinpath(vim.fn.stdpath "state", "avante"))
end

---Nome da pasta do projeto no armazenamento do avante (mesma regra de
---avante/path.lua: "/" vira "__" e o resto que não é alfanumérico vira "_").
---@param root string
---@return string
function M.project_dirname(root)
  return (root:gsub("/", "__"):gsub("[^A-Za-z0-9._]", "_"))
end

---------------------------------------------------------------------------
-- Índice
---------------------------------------------------------------------------

---@class AstrographSession
---@field key string        "<pasta do projeto>/<arquivo>.json"
---@field root? string      pasta do projeto (como o avante a enxerga)
---@field title? string
---@field provider? string
---@field model? string
---@field updated? integer  os.time()
---@field messages? integer

local index ---@type table<string, AstrographSession>?
local dirty, flush_timer = false, nil
local removed = {} ---@type table<string, true> apagadas no disco desde a última gravação

local function index_path() return vim.fs.joinpath(M.storage(), INDEX) end

---@return table<string, AstrographSession>
local function read_disk()
  local f = io.open(index_path(), "r")
  if not f then return {} end
  local ok, data = pcall(vim.json.decode, f:read "*a")
  f:close()
  return ok and type(data) == "table" and type(data.sessions) == "table" and data.sessions or {}
end

---@return table<string, AstrographSession>
function M.index()
  if not index then index = read_disk() end
  return index
end

function M.flush()
  if flush_timer then
    flush_timer:stop()
    flush_timer:close()
    flush_timer = nil
  end
  if not dirty or not index then return end
  -- outro editor aberto pode ter gravado entradas: mescla, vence a mais recente
  -- (conversas que esta lista viu apagadas no disco não voltam)
  for key, s in pairs(read_disk()) do
    local mine = index[key]
    if not removed[key] and (not mine or (tonumber(s.updated) or 0) > (tonumber(mine.updated) or 0)) then index[key] = s end
  end
  vim.fn.mkdir(M.storage(), "p")
  local tmp = index_path() .. ".tmp"
  local f = io.open(tmp, "w")
  if not f then return end -- segue sujo: tenta de novo na próxima gravação
  dirty, removed = false, {}
  f:write(vim.json.encode { version = 1, sessions = index })
  f:close()
  -- troca atômica; se o destino estiver preso (Windows), tira o antigo e tenta de
  -- novo. O índice é só cache: no pior caso a lista é refeita a partir do disco.
  if not uv.fs_rename(tmp, index_path()) then
    os.remove(index_path())
    if not uv.fs_rename(tmp, index_path()) then os.remove(tmp) end
  end
end

-- o avante grava a cada mensagem (milhares numa conversa longa): o índice vai
-- para o disco no máximo a cada 1,5 s
local function schedule_flush()
  dirty = true
  if flush_timer then return end
  flush_timer = uv.new_timer()
  flush_timer:start(1500, 0, vim.schedule_wrap(M.flush))
end

---Agente e modelo do avante agora (`model` pode faltar: nem todo agente informa).
---@return string? provider, string? model
function M.current_model()
  local ok, Config = pcall(require, "avante.config")
  if not ok then return nil end
  local provider = Config.provider
  local model
  if Config.acp_providers and Config.acp_providers[provider] then
    local sok, sidebar = pcall(function() return require("avante").get(false) end)
    local options = sok and sidebar and sidebar.acp_client and sidebar.acp_client.config_options
    for _, opt in ipairs(options or {}) do
      if opt.category == "model" then
        for _, v in ipairs(opt.options or {}) do
          if v.value == opt.currentValue then model = v.name end
        end
        model = model or opt.currentValue
      end
    end
  else
    local p = Config.providers and Config.providers[provider]
    model = type(p) == "table" and p.model or nil
  end
  return provider, model
end

local function first_line(s, max)
  s = tostring(s or ""):match "^%s*([^\n]*)" or ""
  if #s > max then s = vim.fn.strcharpart(s, 0, max - 1) .. "…" end
  return s
end

---Marca a conversa com o modelo e atualiza o índice (chamado antes de cada
---gravação do avante).
---@param bufnr integer
---@param history table
function M.tag(bufnr, history)
  if type(history) ~= "table" or not history.filename then return end
  local tag = type(history.astrograph) == "table" and history.astrograph or {}
  local provider, model = M.current_model()
  -- o modelo de quem COMEÇOU a conversa fica; os que vierem depois entram em
  -- `others` (trocar de modelo no meio não reescreve a origem da conversa)
  if provider then
    if not tag.provider then
      tag.provider, tag.model = provider, model
    elseif tag.provider == provider and not tag.model and model then
      tag.model = model -- o ACP só informa o modelo com a sidebar aberta
    elseif model and (tag.provider ~= provider or tag.model ~= model) then
      local label = provider .. " · " .. model
      tag.others = tag.others or {}
      if not vim.tbl_contains(tag.others, label) then table.insert(tag.others, label) end
    end
    history.astrograph = tag
  end
  local root = require("avante.utils").root.get { buf = bufnr }
  if not root or root == "" then return end
  local key = M.project_dirname(root) .. "/" .. history.filename
  M.index()[key] = {
    key = key,
    root = root,
    title = first_line(history.title, 120),
    provider = tag.provider,
    model = tag.model,
    others = tag.others,
    updated = os.time(),
    messages = type(history.messages) == "table" and #history.messages or nil,
  }
  schedule_flush()
end

local warned = false
---Liga a tag nas gravações do avante (idempotente). Se o avante mudar e o
---encaixe não pegar, avisa uma vez (em vez de a tag sumir calada).
function M.setup()
  local ok, Path = pcall(require, "avante.path")
  if not ok or type(Path.history) ~= "table" or type(Path.history.save) ~= "function" then
    if not warned then
      warned = true
      notify("Sessões: esta versão do avante mudou a gravação do histórico; a tag do modelo está desligada.", vim.log.levels.WARN)
    end
    return
  end
  if Path.history._astrograph_sessions then return end
  local save = Path.history.save
  Path.history.save = function(bufnr, history)
    local tok, err = pcall(M.tag, bufnr, history)
    if not tok and not warned then
      warned = true
      vim.schedule(function() notify("Sessões: falha ao marcar o modelo da conversa: " .. tostring(err), vim.log.levels.WARN) end)
    end
    return save(bufnr, history)
  end
  Path.history._astrograph_sessions = true
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = vim.api.nvim_create_augroup("astrograph_sessions", { clear = true }),
    callback = M.flush,
  })
end

---------------------------------------------------------------------------
-- Lista
---------------------------------------------------------------------------

---Pasta do projeto que o próprio avante guardou (metadata.json, `project_root`).
---@param hdir string pasta history/ do projeto
---@return string?
local function project_root(hdir)
  local f = io.open(vim.fs.joinpath(hdir, "metadata.json"), "r")
  if not f then return nil end
  local ok, meta = pcall(vim.json.decode, f:read "*a")
  f:close()
  local root = ok and type(meta) == "table" and meta.project_root or nil
  return type(root) == "string" and root ~= "" and root or nil
end

---Lê título e tag de uma conversa pequena (as grandes não são abertas).
local function peek(path, size)
  if size > PEEK then return nil end
  local f = io.open(path, "r")
  if not f then return nil end
  local ok, h = pcall(vim.json.decode, f:read "*a")
  f:close()
  if not ok or type(h) ~= "table" then return nil end
  local tag = type(h.astrograph) == "table" and h.astrograph or {}
  return {
    title = first_line(h.title, 120),
    provider = tag.provider,
    model = tag.model,
    messages = type(h.messages) == "table" and #h.messages or nil,
  }
end

---@class AstrographSessionItem: AstrographSession
---@field dir string
---@field filename string
---@field path string
---@field size integer
---@field mtime integer

---Todas as conversas guardadas, da mais recente para a mais antiga.
---@return AstrographSessionItem[]
function M.list()
  local base = vim.fs.joinpath(M.storage(), "projects")
  local idx = M.index()
  local out, seen = {}, {}
  for dir, kind in vim.fs.dir(base) do
    if kind == "directory" then
      local hdir = vim.fs.joinpath(base, dir, "history")
      local root ---@type string|false|nil
      for name, k in vim.fs.dir(hdir) do
        if k == "file" and name:match "%.json$" and name ~= "metadata.json" then
          local path = vim.fs.joinpath(hdir, name)
          local st = uv.fs_stat(path)
          local key = dir .. "/" .. name
          seen[key] = true
          if st and st.size > 2 then -- "{}" = conversa vazia
            local s = idx[key]
            if not s then
              -- conversa de antes do índice: lê uma vez (só as pequenas) e guarda no
              -- índice, para a próxima abertura da lista não ler de novo
              if root == nil then root = project_root(hdir) or false end
              s = peek(path, st.size) or {}
              s.key, s.root, s.old, s.updated = key, root or nil, true, st.mtime.sec
              idx[key] = s
              dirty = true
            end
            local item = vim.tbl_extend("force", {}, s) ---@type AstrographSessionItem
            item.dir, item.filename, item.path = dir, name, path
            item.size, item.mtime = st.size, st.mtime.sec
            if item.messages ~= 0 then out[#out + 1] = item end
          end
        end
      end
    end
  end
  -- conversa apagada no disco sai do índice
  for key in pairs(idx) do
    if not seen[key] then
      idx[key], removed[key] = nil, true
      dirty = true
    end
  end
  if dirty then schedule_flush() end
  table.sort(out, function(a, b) return a.mtime > b.mtime end)
  return out
end

---Texto da tag: "claude-code · Opus", "claude-code · Opus +1" (trocou de modelo
---no meio), "claude-code · modelo ?" (o agente não informou) ou "sem tag (antiga)".
---@param s AstrographSession
---@return string
function M.tag_label(s)
  if not s.provider then return s.old and "sem tag (antiga)" or "sem tag" end
  local label = s.provider .. " · " .. (s.model or "modelo ?")
  if type(s.others) == "table" and #s.others > 0 then label = label .. " +" .. #s.others end
  return label
end

local function mb(n) return ("%.1f MB"):format(n / 1024 / 1024) end

local function project_label(s)
  if s.root then return vim.fn.fnamemodify(vim.fs.normalize(s.root), ":~") end
  return s.dir:gsub("^.*__", "") .. " (pasta não identificada)"
end

---------------------------------------------------------------------------
-- Retomar
---------------------------------------------------------------------------

local function under(path, root)
  local a, b = vim.fs.normalize(path):lower(), vim.fs.normalize(root):lower()
  return a == b or a:sub(1, #b + 1) == b .. "/"
end

M.MANY = 1000 -- mensagens: acima disso também pede confirmação

---Abre a conversa no avante, na pasta do projeto dela. Valida tudo antes de
---mudar pasta, buffer ou agente; se o avante não enxergar o projeto, desfaz.
---@param s AstrographSessionItem
---@param confirmed? boolean
function M.resume(s, confirmed)
  if not s.root then
    return notify(
      ("Não sei em que pasta ficou o projeto desta conversa (%s). Abra o projeto e use :AvanteHistory."):format(s.dir),
      vim.log.levels.WARN
    )
  end
  if M.project_dirname(s.root) ~= s.dir then
    return notify(("A pasta guardada (%s) não corresponde a esta conversa (%s)."):format(s.root, s.dir), vim.log.levels.WARN)
  end
  if not uv.fs_stat(s.root) then return notify("A pasta do projeto não existe mais: " .. s.root, vim.log.levels.WARN) end
  if (s.size > M.HEAVY or (s.messages or 0) > M.MANY) and not confirmed then
    -- a opção segura vem primeiro
    return vim.ui.select({ "Cancelar", "Abrir mesmo assim (pode travar o editor)" }, {
      prompt = ("Conversa grande (%s, %s mensagens)"):format(mb(s.size), s.messages or "?"),
    }, function(_, i)
      if i == 2 then M.resume(s, true) end
    end)
  end

  pcall(function() require("lazy").load { plugins = { "avante.nvim" } } end)
  local avante = require "avante"
  if avante.is_sidebar_open() then avante.close_sidebar() end

  local old_cwd, old_buf = uv.cwd(), vim.api.nvim_get_current_buf()
  local moved = vim.fs.normalize(old_cwd or ""):lower() ~= vim.fs.normalize(s.root):lower()
  if moved then vim.cmd.cd(vim.fn.fnameescape(s.root)) end
  -- buffer de outro projeto faria o avante procurar a conversa no lugar errado
  local name = vim.api.nvim_buf_get_name(0)
  if vim.bo.buftype ~= "" or (name ~= "" and not under(name, s.root)) then pcall(vim.cmd.enew) end
  local buf = vim.api.nvim_get_current_buf()

  local root = require("avante.utils").root.get { buf = buf }
  if M.project_dirname(root) ~= s.dir then
    if buf ~= old_buf and vim.api.nvim_buf_is_valid(old_buf) then vim.api.nvim_set_current_buf(old_buf) end
    if moved and old_cwd then vim.cmd.cd(vim.fn.fnameescape(old_cwd)) end
    return notify(
      ("O avante enxerga outro projeto aqui (%s). Abra um arquivo da pasta %s e use :AvanteHistory."):format(root, s.root),
      vim.log.levels.WARN
    )
  end

  -- agente da conversa (a sessão ACP só retoma no mesmo agente)
  local Config = require "avante.config"
  local switched
  if s.provider and s.provider ~= Config.provider then
    if (Config.acp_providers or {})[s.provider] or (Config.providers or {})[s.provider] then
      local ok, err = pcall(require("avante.api").switch_provider, s.provider, false)
      if ok then
        switched = s.provider
      else
        notify(("Não consegui trocar para %s (%s); abrindo com %s."):format(s.provider, tostring(err), Config.provider), vim.log.levels.WARN)
      end
    else
      notify(("A conversa foi feita com %s, que não está configurado; abrindo com %s."):format(s.provider, Config.provider))
    end
  end

  require("avante.path").history.save_latest_filename(buf, s.filename)
  avante.open_sidebar {}
  local what = {}
  if moved then what[#what + 1] = "pasta: " .. vim.fn.fnamemodify(s.root, ":~") end
  if switched then what[#what + 1] = "agente: " .. switched end
  if #what > 0 then notify("Conversa retomada (" .. table.concat(what, " · ") .. ")") end
end

---------------------------------------------------------------------------
-- Tela
---------------------------------------------------------------------------

function M.pick()
  pcall(function() require("lazy").load { plugins = { "avante.nvim" } } end)
  local list = M.list()
  if #list == 0 then return notify "Nenhuma conversa guardada ainda." end

  local items = {}
  for i, s in ipairs(list) do
    local when = os.date("%d/%m %H:%M", s.mtime)
    local heavy = s.size > M.HEAVY
    local title = s.title ~= nil and s.title ~= "" and s.title or (heavy and "(conversa grande: título não lido)" or "(sem título)")
    items[i] = {
      idx = i,
      session = s,
      text = table.concat({ title, project_label(s), M.tag_label(s) }, " "),
      when = when,
      title = title,
      heavy = heavy,
      preview = {
        ft = "markdown",
        text = table.concat({
          "# " .. title,
          "",
          "- **Projeto:** " .. project_label(s),
          "- **Modelo:** " .. M.tag_label(s),
          "- **Última mensagem:** " .. os.date("%d/%m/%Y %H:%M", s.mtime),
          "- **Mensagens:** " .. tostring(s.messages or "?"),
          "- **Tamanho:** " .. mb(s.size) .. (heavy and "  ⚠ grande: pode travar ao abrir" or ""),
          "- **Arquivo:** `" .. s.path .. "`",
        }, "\n"),
      },
    }
  end

  local ok, snacks = pcall(require, "snacks")
  if ok and snacks.picker then
    snacks.picker.pick {
      title = "Sessões do agente (todos os projetos)",
      items = items,
      format = function(item)
        local s = item.session
        return {
          { item.when, "Comment" },
          { "  " },
          { ("%-22s"):format(M.tag_label(s)), s.provider and "Special" or "Comment" },
          { "  " },
          { project_label(s), "Directory" },
          { "  " },
          { item.title },
          { item.heavy and ("  ⚠ " .. mb(s.size)) or "", "WarningMsg" },
        }
      end,
      preview = "preview",
      confirm = function(picker, item)
        picker:close()
        if item then vim.schedule(function() M.resume(item.session) end) end
      end,
    }
  else
    vim.ui.select(items, {
      prompt = "Sessões do agente",
      format_item = function(item)
        return ("%s  [%s]  %s  %s"):format(item.when, M.tag_label(item.session), project_label(item.session), item.title)
      end,
    }, function(item)
      if item then M.resume(item.session) end
    end)
  end
end

return M
