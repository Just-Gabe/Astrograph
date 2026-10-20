-- `:Astrograph atualizar` — atualização que não deixa o editor pior do que estava.
--
-- Transacional: ou termina com a versão nova funcionando, ou volta exatamente
-- para onde estava. Tudo em segundo plano (o editor segue usável).
--
--   1. trava (um editor atualiza por vez) e confere git e rede (`fetch`); sem rede
--      não mexe em nada;
--   2. guarda o que o usuário alterou (`git stash --include-untracked`), inclusive
--      arquivos soltos que atrapalhariam um arquivo novo da versão;
--   3. avança o clone: fast-forward; se o histórico divergiu (reescrito lá),
--      num clone do instalador (raso) guarda o antigo no ramo
--      `astrograph-backup-<data>` e segue; num clone de desenvolvimento, recusa;
--   4. sincroniza os plugins (lazy-lock) e recompila o app em Rust só se o código
--      dele mudou (o executável novo entra por rename, funciona com o antigo aberto);
--   5. confere que a config sobe (nvim headless); se não subir, desfaz tudo;
--   6. devolve as alterações do usuário; se conflitarem com a versão nova, ficam
--      guardadas no stash e a versão nova fica limpa.

local M = {}

local uv = vim.uv or vim.loop

M.TIMEOUT = 120000 -- ms por comando
M.LOCK_STALE = 30 * 60 -- s: trava mais velha que isto é de um editor que morreu
M.BACKUP_PREFIX = "astrograph-backup-"

local GIT_ENV = { GIT_TERMINAL_PROMPT = "0", GIT_OPTIONAL_LOCKS = "0", LC_ALL = "C", LANG = "C" }

local function notify(msg, level) vim.notify(msg, level or vim.log.levels.INFO, { title = "Astrograph" }) end

---------------------------------------------------------------------------
-- Comandos (assíncronos, escritos em estilo sequencial dentro de uma corrotina)
---------------------------------------------------------------------------

---@return table r { code, stdout, stderr }
local function sh(cmd, opts)
  local co = coroutine.running()
  opts = vim.tbl_extend("force", { text = true, timeout = M.TIMEOUT, env = GIT_ENV }, opts or {})
  local started = pcall(vim.system, cmd, opts, function(r)
    vim.schedule(function() coroutine.resume(co, r) end)
  end)
  if not started then return { code = 127, stdout = "", stderr = "comando não encontrado: " .. tostring(cmd[1]) } end
  return coroutine.yield()
end

local function out(r) return vim.trim(r.stdout or "") end

---------------------------------------------------------------------------
-- Trava
---------------------------------------------------------------------------

local function lock_path(root) return vim.fs.joinpath(root, ".git", "astrograph-update.lock") end

---@return boolean ok
local function lock(root)
  local path = lock_path(root)
  local st = uv.fs_stat(path)
  if st and os.time() - st.mtime.sec < M.LOCK_STALE then return false end
  local f = io.open(path, "w")
  if not f then return true end -- sem permissão de escrita no .git: segue sem trava
  f:write(tostring(uv.os_getpid()))
  f:close()
  return true
end

local function unlock(root) os.remove(lock_path(root)) end

---------------------------------------------------------------------------
-- Passos de depois do git (substituíveis nos testes)
---------------------------------------------------------------------------

---Sincroniza os plugins com o lazy-lock.json, sem janela.
---@param cb fun(ok: boolean)
local function lazy_restore(cb)
  local ok, lazy = pcall(require, "lazy")
  if not ok then return cb(true) end
  local done = false
  local function finish(v)
    if done then return end
    done = true
    cb(v)
  end
  local started, runner = pcall(lazy.restore, { wait = false, show = false })
  if not started or not runner or type(runner.wait) ~= "function" then return finish(started) end
  pcall(runner.wait, runner, function() finish(true) end)
  vim.defer_fn(function() finish(true) end, 10 * 60 * 1000)
end

local NATIVE = {
  { crate = "astrograph-agent", bin = "astrograph-agent" },
  { crate = "astrograph-ui", bin = "astrograph" },
}

local function exe(name) return name .. (vim.fn.has "win32" == 1 and ".exe" or "") end

---Troca o executável sem apagar o que está aberto: o antigo vira `.old` (no
---Windows renomear um .exe em uso funciona; sobrescrever, não).
local function swap_binary(new, target)
  local old = target .. ".old"
  os.remove(old)
  if uv.fs_stat(target) then uv.fs_rename(target, old) end
  if not uv.fs_rename(new, target) then
    if uv.fs_stat(old) then uv.fs_rename(old, target) end -- volta o antigo
    return false
  end
  return true
end

---Recompila só o que mudou em `prototype/<crate>` entre `old` e `new` (ou falta).
---Sem `cargo`, não faz nada: o app antigo continua valendo.
---@param root string
---@param old string
---@param new string
---@return string[] rebuilt, string[] failed
local function native(root, old, new)
  local rebuilt, failed = {}, {}
  if vim.fn.executable "cargo" ~= 1 then return rebuilt, failed end
  for _, n in ipairs(NATIVE) do
    local manifest = vim.fs.joinpath(root, "prototype", n.crate, "Cargo.toml")
    local target = vim.fs.joinpath(root, "bin", exe(n.bin))
    if uv.fs_stat(manifest) then
      local changed = true
      if uv.fs_stat(target) then
        local d = sh { "git", "-C", root, "diff", "--quiet", old, new, "--", "prototype/" .. n.crate }
        changed = d.code ~= 0 -- 1 = mudou; erro = assume que mudou
      end
      if changed then
        local cmd = { "cargo", "build", "--release", "--manifest-path", manifest }
        if vim.fn.has "win32" ~= 1 and vim.fn.executable "nice" == 1 then cmd = vim.list_extend({ "nice", "-n", "19" }, cmd) end
        local b = sh(cmd, { timeout = 30 * 60 * 1000 })
        local built = vim.fs.joinpath(root, "prototype", n.crate, "target", "release", exe(n.crate))
        local copy = target .. ".new"
        vim.fn.mkdir(vim.fs.dirname(target), "p")
        if b.code == 0 and uv.fs_stat(built) and uv.fs_copyfile(built, copy) and swap_binary(copy, target) then
          rebuilt[#rebuilt + 1] = n.crate
        else
          os.remove(copy)
          failed[#failed + 1] = n.crate
        end
      end
    end
  end
  return rebuilt, failed
end

---Sobe a config num nvim headless e procura erro. Estourar o tempo não conta
---como falha (máquina lenta não pode desfazer uma atualização boa).
---@return boolean ok, string detail
local function verify()
  local r = sh({ vim.v.progpath, "--headless", "+lua io.stdout:write('ASTROGRAPH_OK')", "+qa" }, { timeout = 90000, env = {} })
  local text = (r.stdout or "") .. "\n" .. (r.stderr or "")
  if r.signal and r.signal ~= 0 then return true, "tempo esgotado" end
  if not text:find("ASTROGRAPH_OK", 1, true) then return false, vim.trim(text):sub(1, 600) end
  local bad = text:match "[^\n]*E5%d%d%d[^\n]*" or text:match "[^\n]*module '[^']+' not found[^\n]*" or text:match "[^\n]*Error detected while processing[^\n]*"
  if bad then return false, vim.trim(bad) end
  return true, ""
end

M.steps = { restore = lazy_restore, native = native, verify = verify }

---------------------------------------------------------------------------
-- Atualização
---------------------------------------------------------------------------

---@class AstrographUpdateResult
---@field status string   atualizado | ja-atualizado | revertido | sem-rede | divergiu | ocupado | sem-git | erro
---@field message string
---@field old? string
---@field new? string
---@field stash? string   nome do stash com as alterações do usuário que não puderam voltar
---@field backup? string  ramo com a versão anterior (histórico divergiu)
---@field rebuilt? string[]

---@param opts? { root?: string, remote?: string, branch?: string, force?: boolean }
---@param done fun(result: AstrographUpdateResult)
function M.run(opts, done)
  opts = opts or {}
  local co
  co = coroutine.create(function()
    local result = M._run(opts)
    done(result)
  end)
  local ok, err = coroutine.resume(co)
  if not ok then done { status = "erro", message = "Falha inesperada na atualização: " .. tostring(err) } end
end

---@param opts table
---@return AstrographUpdateResult
function M._run(opts)
  local root = opts.root or require("astrograph.commands").root()
  if not root then
    return {
      status = "sem-git",
      message = "A config não aponta para um clone git do Astrograph. Rode o instalador de novo (ele atualiza e religa tudo).",
    }
  end
  if vim.fn.executable "git" ~= 1 then return { status = "sem-git", message = "O git não está instalado (ou fora do PATH)." } end
  if not lock(root) then
    return { status = "ocupado", message = "Outro Neovim já está atualizando o Astrograph. Espere terminar." }
  end

  local function g(...) return sh(vim.list_extend({ "git", "-C", root }, { ... })) end
  local remote = opts.remote or "origin"
  local stamp = os.date "%Y%m%d-%H%M%S"
  local result = { status = "erro", message = "" }

  local ok, err = pcall(function()
    local head = g("rev-parse", "HEAD")
    if head.code ~= 0 then
      result = { status = "erro", message = "Não consegui ler o repositório: " .. vim.trim(head.stderr or "") }
      return
    end
    local old = out(head)
    local shallow = out(g("rev-parse", "--is-shallow-repository")) == "true"

    -- ramo a seguir: o configurado, o upstream do atual, ou o main
    local branch = opts.branch
    if not branch then
      local up = out(g("rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"))
      branch = up:match("^[^/]+/(.+)$") or "main"
    end

    -- 1. busca (nada mudou ainda se falhar)
    local fetch = { "fetch", "--no-tags", "--quiet" }
    if shallow then vim.list_extend(fetch, { "--depth=1" }) end
    vim.list_extend(fetch, { remote, branch })
    local f = g(unpack(fetch))
    if f.code ~= 0 then
      local why = vim.trim(f.stderr or "")
      local offline = why:match "[Cc]ould not resolve" or why:match "unable to access" or why:match "Failed to connect" or why:match "timed out" or f.signal ~= 0
      result = {
        status = offline and "sem-rede" or "erro",
        message = (offline and "Sem conexão com " .. remote .. " (nada foi alterado)." or "O git não conseguiu buscar a versão nova (nada foi alterado):") .. (offline and "" or "\n" .. why),
        old = old,
      }
      return
    end
    local new = out(g("rev-parse", "FETCH_HEAD"))
    if new == old then
      result = { status = "ja-atualizado", message = "O Astrograph já está na última versão.", old = old, new = new }
      return
    end

    -- 2. guarda o que é do usuário (o Lazy reescreve o lazy-lock sozinho: descarta)
    g("checkout", "--", "template/lazy-lock.json")
    local dirty = out(g("status", "--porcelain=v1", "--untracked-files=normal"))
    local stashed = false
    if dirty ~= "" then
      local s = g("stash", "push", "--include-untracked", "-m", "astrograph-update-" .. stamp)
      if s.code ~= 0 then
        result = { status = "erro", message = "Não consegui guardar suas alterações antes de atualizar (nada foi alterado):\n" .. vim.trim(s.stderr or ""), old = old }
        return
      end
      stashed = not out(s):match "No local changes"
    end
    local function stash_name() return "astrograph-update-" .. stamp end

    local function restore_old()
      g("reset", "--hard", old)
      if stashed then g("stash", "pop", "--quiet") end
    end

    -- 3. avança
    local ancestor = g("merge-base", "--is-ancestor", old, new).code == 0
    local backup
    if ancestor then
      local m = g("merge", "--ff-only", "--quiet", new)
      if m.code ~= 0 then
        restore_old()
        result = { status = "erro", message = "Não consegui avançar para a versão nova (voltei ao que estava):\n" .. vim.trim(m.stderr or ""), old = old }
        return
      end
    else
      if not (shallow or opts.force) then
        if stashed then g("stash", "pop", "--quiet") end
        result = {
          status = "divergiu",
          message = "Este clone tem histórico próprio (commits que não estão em " .. remote .. "/" .. branch .. "). Não vou sobrescrever: atualize com git (merge ou rebase).",
          old = old,
        }
        return
      end
      backup = M.BACKUP_PREFIX .. stamp
      g("branch", "-f", backup, old)
      -- fica um ponto de restauração por atualização, só os 3 últimos
      local all = vim.split(out(g("branch", "--list", M.BACKUP_PREFIX .. "*", "--format=%(refname:short)")), "\n", { trimempty = true })
      table.sort(all)
      for i = 1, #all - 3 do
        g("branch", "-D", all[i])
      end
      local r = g("reset", "--hard", new)
      if r.code ~= 0 then
        restore_old()
        result = { status = "erro", message = "Não consegui trocar para a versão nova (voltei ao que estava):\n" .. vim.trim(r.stderr or ""), old = old }
        return
      end
    end

    -- 4. plugins, app em Rust e conferência (com a árvore limpa na versão nova)
    do
      local co = coroutine.running()
      M.steps.restore(function() vim.schedule(function() coroutine.resume(co) end) end)
      coroutine.yield()
    end
    local rebuilt, failed = M.steps.native(root, old, new)
    local good, detail = M.steps.verify()
    if not good then
      restore_old()
      do
        local co = coroutine.running()
        M.steps.restore(function() vim.schedule(function() coroutine.resume(co) end) end)
        coroutine.yield()
      end
      result = {
        status = "revertido",
        message = "A versão nova não subiu aqui, então voltei para a anterior (nada foi perdido):\n" .. detail,
        old = old,
        new = new,
      }
      return
    end

    -- 6. devolve as alterações do usuário
    local kept
    if stashed then
      local pop = g("stash", "pop", "--quiet")
      if pop.code ~= 0 then
        g("reset", "--hard", "HEAD") -- tira o meio-merge; o stash continua guardado
        kept = stash_name()
      end
    end

    local log = g("log", "--oneline", "-n", "8", old .. ".." .. new)
    local news = log.code == 0 and out(log) or ""
    local msg = ("Astrograph atualizado (%s → %s)."):format(old:sub(1, 7), new:sub(1, 7))
    if news ~= "" then msg = msg .. "\n" .. news end
    if #rebuilt > 0 then msg = msg .. "\nApp recompilado: " .. table.concat(rebuilt, ", ") .. "." end
    if #failed > 0 then msg = msg .. "\nNão consegui recompilar " .. table.concat(failed, ", ") .. ": o app antigo continua valendo." end
    -- num clone raso o git não enxerga o pai da versão nova: o ramo é só ponto de
    -- restauração; só avisa quando o histórico divergiu de verdade
    if backup and not shallow then msg = msg .. "\nA versão anterior ficou no ramo " .. backup .. "." end
    if kept then
      msg = msg .. "\nSuas alterações conflitaram com a versão nova e ficaram guardadas: git -C \"" .. root .. "\" stash list (" .. kept .. ")."
    end
    msg = msg .. "\nReinicie o Neovim para carregar a versão nova."
    result = { status = "atualizado", message = msg, old = old, new = new, stash = kept, backup = backup, rebuilt = rebuilt }
  end)
  unlock(root)
  if not ok then return { status = "erro", message = "Falha inesperada na atualização: " .. tostring(err) } end
  return result
end

---Executável do serviço em Rust (`<clone>/bin/astrograph-servicos`), se instalado.
---@param root string
---@return string?
function M.servicos(root)
  local p = vim.fs.joinpath(root, "bin", "astrograph-servicos" .. (vim.fn.has "win32" == 1 and ".exe" or ""))
  if vim.env.ASTROGRAPH_ATUALIZAR ~= "lua" and vim.fn.executable(p) == 1 then return p end
end

---Atualiza pelo serviço em Rust (a lógica de verdade; este arquivo é o espelho
---para quando o executável não está instalado).
---@param bin string
---@param root string
---@param done fun(result: AstrographUpdateResult)
function M.run_rust(bin, root, done)
  local started = pcall(vim.system, { bin, "atualizar", "--raiz", root, "--nvim", vim.v.progpath }, {
    text = true,
    timeout = 60 * 60 * 1000,
  }, function(r)
    vim.schedule(function()
      local line
      for l in (r.stdout or ""):gmatch "[^\r\n]+" do
        line = l -- a última linha é o resultado
      end
      local ok, data = pcall(vim.json.decode, line or "")
      if ok and type(data) == "table" and data.status then
        done { status = data.status, message = data.mensagem or "", old = data.antes, new = data.depois, stash = data.stash, backup = data.backup, rebuilt = data.recompilados }
      else
        done { status = "erro", message = "O serviço de atualização terminou sem resultado:\n" .. vim.trim(r.stderr or ""):sub(1, 600) }
      end
    end)
  end)
  if not started then done { status = "erro", message = "Não consegui iniciar " .. bin } end
end

---`:Astrograph atualizar`
function M.atualizar()
  notify "Atualizando o Astrograph em segundo plano…"
  local release = require("astrograph.prepare").track "atualizando"
  local function finish(r)
    release()
    local level = (r.status == "atualizado" or r.status == "ja-atualizado") and vim.log.levels.INFO or vim.log.levels.WARN
    if r.status == "erro" then level = vim.log.levels.ERROR end
    notify(r.message, level)
  end
  local root = require("astrograph.commands").root()
  local bin = root and M.servicos(root)
  if bin then return M.run_rust(bin, root, finish) end
  M.run({}, finish)
end

return M
