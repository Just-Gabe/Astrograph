-- Modo do agente (permissões) unificado: uma escolha, salva no config.json, que
-- vale para o painel nativo, o chat (CodeCompanion), o avante e o terminal.
-- Onde o agente tem o modo, a sessão nasce nele; onde não tem (opencode: só
-- build/plan), o Astrograph imita e avisa.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")

-- config.json do teste numa pasta temporária (nunca a escolha real do usuário)
local tmp = vim.fs.normalize(vim.fn.tempname())
vim.fn.mkdir(tmp, "p")
vim.env.APPDATA, vim.env.LOCALAPPDATA, vim.env.XDG_CONFIG_HOME = tmp, tmp, tmp

local agents = require "astrograph.agents"
local modes = require "astrograph.modes"

-- ids medidos na ponte do Claude Code (0.91) e no `opencode acp` (2.0.31)
local CLAUDE = { "default", "acceptEdits", "plan", "auto", "bypassPermissions" }
local OPENCODE = { "build", "plan" }

-- sem escolha: Manual
H.eq(modes.current(), "manual", "padrão: Manual")
H.eq((modes.resolve(CLAUDE)), "default", "Manual no Claude = default")

-- escolha salva e lida de volta (vale entre sessões)
H.ok(modes.set("acceptEdits", { quiet = true }), "set aceita modo conhecido")
H.eq(agents.read_config().agent_mode, "acceptEdits", "salvo no config.json")
H.eq(modes.current(), "acceptEdits", "modo atual")
H.ok(not modes.set("qualquer", { quiet = true }), "modo desconhecido recusado")
H.eq(modes.current(), "acceptEdits", "recusa não mexe na escolha")

-- resolução por agente
local id, exact = modes.resolve(CLAUDE)
H.eq(id, "acceptEdits", "Claude tem Aceitar edições")
H.ok(exact, "Claude: modo de verdade")
id, exact = modes.resolve(OPENCODE)
H.eq(id, "build", "opencode sem Aceitar edições: build")
H.ok(not exact, "opencode: imitado")
H.eq((modes.resolve(CLAUDE, "bypass")), "bypassPermissions", "Sem permissões no Claude")
H.eq((modes.resolve(OPENCODE, "plan")), "plan", "Plano no opencode")
H.eq((modes.resolve({ { id = "default" }, { id = "auto" } }, "auto")), "auto", "aceita a lista {id,name} do painel")

-- imitação: só aprova sozinho quando o agente não tem o modo
H.ok(not modes.auto_approves("edit", true), "modo de verdade: a tela mostra o que o agente pedir")
H.ok(modes.auto_approves("edit", false), "imitado: edição passa")
H.ok(not modes.auto_approves("execute", false), "imitado: comando pergunta")
H.ok(not modes.auto_approves("edit", false, "manual"), "Manual imitado: pergunta tudo")
H.ok(modes.auto_approves("execute", false, "bypass"), "Sem permissões imitado: tudo")

-- avante: o padrão dele (aprovar TUDO) não vale mais
H.eq(modes.avante_auto_approve(true), false, "avante com o modo de verdade: pergunta o que o agente pedir")
local list = modes.avante_auto_approve(false)
H.ok(type(list) == "table" and vim.tbl_contains(list, "edit") and vim.tbl_contains(list, "write_to_file"), "avante imitado: edições")
H.ok(not vim.tbl_contains(list, "bash") and not vim.tbl_contains(list, "execute"), "avante imitado: comandos não")
H.eq(modes.avante_auto_approve(false, "manual"), false, "avante Manual: nada sozinho")

-- terminal
local claude = agents.by_bin "claude"
local opencode = agents.by_bin "opencode"
H.eq((modes.terminal_cmd(claude)), "claude --permission-mode acceptEdits", "terminal do Claude no modo")
H.eq((modes.terminal_cmd(claude, "manual")), "claude --permission-mode default", "Manual explícito (vence o defaultMode do settings)")
H.eq((modes.terminal_cmd(opencode, "auto")), "opencode --auto", "opencode Auto: --auto")
local cmd, ex = modes.terminal_cmd(opencode, "plan")
H.eq(cmd, "opencode", "opencode Plano: sem opção de linha de comando")
H.ok(not ex, "opencode Plano no terminal: avisado como não aplicado")

-- painel nativo: sessão nasce no modo de verdade, e o 1º pedido espera a troca
local sent = {}
vim.fn.chansend = function(_, data) sent[#sent + 1] = vim.json.decode(data) end
local ag = H.agent()
local st = ag._state
st.job, st.agent = 99, claude
st.queue = { { op = "prompt", text = "oi" } }
H.event(ag, { ev = "ready", mode = "default", modes = { { id = "default", name = "Manual" }, { id = "acceptEdits", name = "Accept edits" }, { id = "plan", name = "Plan" } } })
H.eq(sent[1] and sent[1].op, "mode", "painel: pede a troca de modo antes de tudo")
H.eq(sent[1] and sent[1].mode, "acceptEdits", "painel: para o modo escolhido")
H.eq(#sent, 1, "painel: o pedido espera o modo")
H.event(ag, { ev = "mode", mode = "acceptEdits" })
H.eq(sent[2] and sent[2].op, "prompt", "painel: modo confirmado, pedido sai")
H.ok(st.ready and st.exact, "painel: pronto, modo de verdade")
-- trocar o modo com a sessão aberta leva a troca para ela
modes.set("plan", { quiet = true })
H.eq(sent[3] and sent[3].mode, "plan", "painel: troca levada à sessão aberta")
-- permissão com o modo de verdade: aparece para você
H.event(ag, { ev = "permission", id = 7, tool = "t1", title = "rm x", kind = "execute", options = { { optionId = "ok", name = "Yes", kind = "allow_once" } } })
H.eq(#H.menus, 1, "painel: permissão pedida pelo agente aparece")

-- painel com agente sem o modo (opencode): imita; edição passa sozinha, comando pergunta
modes.set("acceptEdits", { quiet = true })
ag.stop(true)
sent, H.menus = {}, {}
st.job, st.agent, st.ready, st.busy = 98, opencode, false, false
H.event(ag, { ev = "ready", mode = "build", modes = { { id = "build" }, { id = "plan" } } })
H.eq(#sent, 0, "opencode: já está no build, nada a trocar")
H.ok(st.ready and not st.exact, "opencode: imitado")
local opts = { { optionId = "sim", name = "Yes", kind = "allow_once" }, { optionId = "sempre", name = "Always", kind = "allow_always" } }
H.event(ag, { ev = "permission", id = 8, tool = "t2", title = "editar a.lua", kind = "edit", options = opts })
H.eq(sent[1] and sent[1].option, "sim", "opencode imitado: edição aprovada só uma vez")
H.eq(#H.menus, 0, "opencode imitado: edição sem pergunta")
H.event(ag, { ev = "permission", id = 9, tool = "t3", title = "npm test", kind = "execute", options = opts })
H.eq(#H.menus, 1, "opencode imitado: comando pergunta")
H.ok(vim.iter(H.notes):any(function(n) return n:find("opencode não tem o modo Aceitar edições —", 1, true) ~= nil end), "aviso de imitação")

-- chat (CodeCompanion): toda sessão ACP nasce no modo, antes do 1º pedido
local cc_set, cc_approval = {}, {}
local Connection = {
  _establish_session = function(self)
    self.session_id = "s1"
    self._config_options = { { id = "mode", category = "mode", type = "select", currentValue = "default", options = vim.tbl_map(function(v) return { value = v, name = v } end, CLAUDE) } }
    return true
  end,
  set_config_option = function(_, cid, v)
    cc_set[#cc_set + 1] = cid .. "=" .. v
    return true
  end,
}
package.loaded["codecompanion.acp"] = Connection
package.loaded["codecompanion.interactions.chat.tools.approvals"] = { set_mode = function(_, buf, o) cc_approval[buf] = o.mode end }
modes.setup_codecompanion()
modes.set("auto", { quiet = true })
local conn = setmetatable({ chat = { bufnr = 4 }, adapter = { formatted_name = "Claude Code" } }, { __index = Connection })
H.ok(conn:_establish_session(), "chat: sessão abre")
H.eq(cc_set[1], "mode=auto", "chat: sessão nasce no modo")
H.eq(cc_approval[4], "ask", "chat: com o modo de verdade, o chat mostra o que o agente pedir")

-- avante: idem, e a aprovação automática dele segue o modo
local av_cfg = { provider = "claude-code", behaviour = { auto_approve_tool_permissions = true } }
package.loaded["avante.config"] = av_cfg
local av_calls = {}
local ACPClient = {
  create_session = function(self, _, _, cb)
    self.config_options = { { id = "mode", category = "mode", currentValue = "default", options = vim.tbl_map(function(v) return { value = v } end, CLAUDE) } }
    cb("sa", nil)
  end,
  set_config_option = function(_, sid, cid, v, cb)
    av_calls[#av_calls + 1] = sid .. ":" .. cid .. "=" .. v
    cb({}, nil)
  end,
}
ACPClient.load_session = function(self, sid, _, _, cb)
  self.config_options = { { id = "mode", category = "mode", currentValue = "plan", options = vim.tbl_map(function(v) return { value = v } end, CLAUDE) } }
  cb({ sessionId = sid }, nil)
end
package.loaded["avante.libs.acp_client"] = ACPClient
modes.setup_avante()
H.ok(av_cfg.behaviour.auto_approve_tool_permissions ~= true, "avante: não aprova mais tudo por padrão")
local got
ACPClient.create_session(setmetatable({}, { __index = ACPClient }), "/x", {}, function(sid) got = sid end)
vim.wait(500, function() return got ~= nil end)
H.eq(av_calls[1], "sa:mode=auto", "avante: sessão nasce no modo")
H.eq(got, "sa", "avante: a sessão segue depois do modo")
H.eq(av_cfg.behaviour.auto_approve_tool_permissions, false, "avante com o modo de verdade: pergunta o que o agente pedir")
-- conversa retomada (<Leader>AS) que estava em outro modo volta no escolhido
local loaded
ACPClient.load_session(setmetatable({}, { __index = ACPClient }), "sb", "/x", {}, function(r) loaded = r end)
vim.wait(500, function() return loaded ~= nil end)
H.eq(av_calls[2], "sb:mode=auto", "avante: retomada volta no modo escolhido")
H.eq(loaded and loaded.sessionId, "sb", "avante: retomada segue depois do modo")

-- avante: durante a troca de modo de uma sessão aberta, nunca aprova no modo velho
local pending
ACPClient.set_config_option = function(_, _, _, _, cb) pending = cb end
modes.current = function() return "bypass" end
local cfg_open = { config_options = { { id = "mode", category = "mode", currentValue = "default", options = vim.tbl_map(function(v) return { value = v } end, CLAUDE) } } }
modes.apply_avante(setmetatable(cfg_open, { __index = ACPClient }), "sc")
H.eq(av_cfg.behaviour.auto_approve_tool_permissions, false, "avante: pergunta tudo até a troca confirmar")
pending({}, { message = "falhou" })
vim.wait(200, function() return false end)
H.eq(av_cfg.behaviour.auto_approve_tool_permissions, true, "avante: troca falhou em bypass = imitação (avisada)")
-- agente que não responde à troca não prende a conversa
modes.switch_timeout = 100
local seguiu = false
ACPClient.set_config_option = function() end -- nunca responde
modes.apply_avante(setmetatable({ config_options = cfg_open.config_options }, { __index = ACPClient }), "sd", function() seguiu = true end)
vim.wait(1000, function() return seguiu end, 20)
H.ok(seguiu, "avante: sem resposta à troca, a conversa segue (imitação)")
modes.current = nil
package.loaded["astrograph.modes"] = nil
modes = require "astrograph.modes"

-- Sem permissões pede confirmação; "Não" não muda nada
H.menus = {}
modes.set("manual", { quiet = true })
H.pick = nil
H.ok(not modes.set("bypass", { quiet = true }), "bypass: pede confirmação antes")
H.eq(#H.menus, 1, "bypass: menu de confirmação")
H.eq(modes.current(), "manual", "bypass: sem confirmar, nada muda")
H.ok(modes.set("bypass", { quiet = true, confirmed = true }), "bypass: confirmado")
-- e avisa na abertura do nvim enquanto estiver salvo
H.notes = {}
package.loaded["astrograph.modes"] = nil
modes = require "astrograph.modes"
modes.setup()
vim.api.nvim_exec_autocmds("User", { pattern = "VeryLazy" })
H.ok(vim.iter(H.notes):any(function(n) return n:find("Sem permissões", 1, true) ~= nil end), "bypass salvo: aviso na abertura")
modes.set("plan", { quiet = true })

-- painel: só a confirmação do modo pedido solta a fila; sem ela, imita e avisa
ag.stop(true)
sent, H.notes = {}, {}
st.job, st.agent, st.ready, st.busy = 97, claude, false, false
st.queue = { { op = "prompt", text = "x" } }
H.event(ag, { ev = "ready", mode = "default", modes = { { id = "default" }, { id = "plan" } } })
H.event(ag, { ev = "mode", mode = "acceptEdits" })
H.ok(not st.ready, "painel: evento de outro modo não solta a fila")
vim.wait(3500, function() return st.ready end, 50)
H.ok(st.ready and not st.exact, "painel: sem confirmação, segue imitando")
H.ok(vim.iter(H.notes):any(function(n) return n:find("não tem o modo Plano", 1, true) ~= nil end), "painel: sem confirmação, avisa")

-- chat imitado: o aviso diz o que o chat faz de verdade
H.notes = {}
modes.set("acceptEdits", { quiet = true })
modes.apply_codecompanion(setmetatable({ session_id = "s2", chat = { bufnr = 5 }, adapter = { formatted_name = "opencode" },
  _config_options = { { id = "mode", category = "mode", type = "select", currentValue = "build", options = { { value = "build" }, { value = "plan" } } } } }, { __index = Connection }))
H.ok(vim.iter(H.notes):any(function(n) return n:find("o chat pergunta tudo", 1, true) ~= nil end), "chat imitado: aviso verdadeiro")
H.eq(cc_approval[5], "ask", "chat: Aceitar edições imitado pergunta tudo")

H.done()
