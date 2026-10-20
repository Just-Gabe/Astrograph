-- :Astrograph atualizar — transacional: versão nova funcionando ou tudo como estava.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local U = require "astrograph.update"
local uv = vim.uv

local base = vim.fs.normalize(vim.fn.tempname())
vim.fn.mkdir(base, "p")
local function git(dir, ...)
  local r = vim.system({ "git", "-C", dir, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "protocol.file.allow=always", ... }, { text = true }):wait()
  if r.code ~= 0 then error(("git %s: %s"):format(table.concat({ ... }, " "), r.stderr)) end
  return vim.trim(r.stdout or "")
end
local function write(path, text) vim.fn.writefile(vim.split(text, "\n", { plain = true }), path) end
local function read(path) return table.concat(vim.fn.readfile(path), "\n") end

-- upstream: bare + uma cópia de trabalho para publicar
local bare = base .. "/up.git"
local dev = base .. "/dev"
vim.fn.mkdir(bare, "p")
git(bare, "init", "-q", "--bare", "-b", "main")
git(base, "clone", "-q", "file://" .. bare, "dev")
vim.fn.mkdir(dev .. "/template", "p")
write(dev .. "/template/lazy-lock.json", "{}")
write(dev .. "/a.txt", "1")
write(dev .. "/b.txt", "1")
git(dev, "checkout", "-q", "-b", "main")
git(dev, "add", "-A")
git(dev, "commit", "-q", "-m", "um")
git(dev, "push", "-q", "origin", "main")
local n = 1
local function publish(file, text)
  write(dev .. "/" .. file, text)
  git(dev, "add", "-A")
  n = n + 1
  git(dev, "commit", "-q", "-m", "c" .. n)
  git(dev, "push", "-q", "-f", "origin", "main")
end

-- passos de fora do git, trocados por falsos
local verify_ok = true
U.steps = {
  restore = function(cb) cb(true) end,
  native = function() return {}, {} end,
  verify = function() return verify_ok, verify_ok and "" or "module 'x' not found" end,
}

local function clone(name, shallow)
  local dir = base .. "/" .. name
  git(base, "clone", "-q", shallow and "--depth=1" or "--no-local", "file://" .. bare, name)
  vim.fn.mkdir(dir .. "/bin", "p")
  return dir
end
local function update(root, extra)
  local res
  U.run(vim.tbl_extend("force", { root = root, branch = "main" }, extra or {}), function(r) res = r end)
  H.ok(vim.wait(60000, function() return res ~= nil end, 20), "atualização terminou")
  return res
end

-- 1. avanço limpo (clone raso, como o do instalador)
local c = clone("raso", true)
publish("a.txt", "2")
local r = update(c)
H.eq(r.status, "atualizado", "avanço limpo: " .. tostring(r.message))
H.eq(read(c .. "/a.txt"), "2", "arquivo novo chegou")
H.eq(git(c, "rev-parse", "HEAD"), r.new, "HEAD na versão nova")
H.eq(update(c).status, "ja-atualizado", "de novo: já atualizado")

-- 2. alteração do usuário em outro arquivo volta depois
write(c .. "/b.txt", "meu")
publish("a.txt", "3")
r = update(c)
H.eq(r.status, "atualizado", "com alteração local")
H.eq(read(c .. "/a.txt"), "3", "versão nova")
H.eq(read(c .. "/b.txt"), "meu", "alteração do usuário preservada")
H.eq(git(c, "stash", "list"), "", "stash vazio")

-- 3. alteração que conflita: árvore limpa na versão nova, alteração no stash
write(c .. "/a.txt", "meu a")
publish("a.txt", "4")
r = update(c)
H.eq(r.status, "atualizado", "conflito não é falha")
H.eq(read(c .. "/a.txt"), "4", "versão nova prevalece")
H.ok(r.stash ~= nil, "avisa onde ficou a alteração")
H.ok(git(c, "stash", "list"):find("astrograph-update", 1, true) ~= nil, "alteração guardada no stash")
H.ok(r.message:find("stash", 1, true) ~= nil, "mensagem explica")
git(c, "checkout", "-q", "--", ".")
git(c, "stash", "drop", "-q")
write(c .. "/b.txt", "1") -- volta ao do upstream
git(c, "checkout", "-q", "--", ".")

-- 4. arquivo solto que atrapalharia um arquivo novo da versão
write(c .. "/novo.txt", "solto")
publish("novo.txt", "do upstream")
r = update(c)
H.eq(r.status, "atualizado", "arquivo solto no caminho: " .. tostring(r.message))
H.eq(read(c .. "/novo.txt"), "do upstream", "arquivo do upstream")
H.ok(git(c, "stash", "list") ~= "", "o solto ficou guardado")
git(c, "stash", "drop", "-q")

-- 5. histórico reescrito lá: clone raso segue, com ponto de restauração
git(dev, "reset", "-q", "--hard", "HEAD~3")
publish("a.txt", "reescrito")
r = update(c)
H.eq(r.status, "atualizado", "histórico reescrito (raso): " .. tostring(r.message))
H.eq(read(c .. "/a.txt"), "reescrito", "conteúdo do upstream")
H.ok(git(c, "branch", "--list", "astrograph-backup-*") ~= "", "ponto de restauração criado")

-- 6. clone de desenvolvimento (completo) com commit próprio: recusa, não perde nada
local full = clone("completo", false)
write(full .. "/mine.txt", "meu commit")
git(full, "add", "-A")
git(full, "commit", "-q", "-m", "meu")
local mine = git(full, "rev-parse", "HEAD")
publish("a.txt", "outro")
r = update(full)
H.eq(r.status, "divergiu", "clone de desenvolvimento: " .. tostring(r.message))
H.eq(git(full, "rev-parse", "HEAD"), mine, "commit do usuário intacto")

-- 7. a versão nova não sobe: volta ao que estava, alteração do usuário também
local v = clone("verifica", true)
local before = git(v, "rev-parse", "HEAD")
write(v .. "/b.txt", "editado")
publish("a.txt", "quebra")
verify_ok = false
r = update(v)
verify_ok = true
H.eq(r.status, "revertido", "verificação falhou: " .. tostring(r.message))
H.eq(git(v, "rev-parse", "HEAD"), before, "voltou à versão anterior")
H.eq(read(v .. "/b.txt"), "editado", "alteração do usuário de volta")
H.ok(r.message:find("module 'x' not found", 1, true) ~= nil, "motivo na mensagem")
r = update(v)
H.eq(r.status, "atualizado", "depois de corrigido, atualiza")

-- 8. sem rede: nada muda
local off = clone("offline", true)
git(off, "remote", "set-url", "origin", "http://127.0.0.1:9/x.git")
local h = git(off, "rev-parse", "HEAD")
r = update(off)
H.eq(r.status, "sem-rede", "sem rede: " .. tostring(r.message))
H.eq(git(off, "rev-parse", "HEAD"), h, "nada mudou")

-- 9. trava: outro editor atualizando; trava velha é ignorada
local lk = clone("trava", true)
publish("a.txt", "para a trava")
write(lk .. "/.git/astrograph-update.lock", "123")
H.eq(update(lk).status, "ocupado", "trava ativa")
local old = os.time() - 3 * 3600
uv.fs_utime(lk .. "/.git/astrograph-update.lock", old, old)
H.eq(update(lk).status, "atualizado", "trava velha não segura")
H.ok(uv.fs_stat(lk .. "/.git/astrograph-update.lock") == nil, "trava solta no fim")

-- 10. sem clone git
H.eq(update(nil, { root = base .. "/nada" }).status ~= "atualizado", true, "pasta sem git não atualiza")
H.done()
