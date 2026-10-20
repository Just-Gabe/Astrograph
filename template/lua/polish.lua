-- Roda por último no setup: ajustes que não cabem nos specs de plugins.
--
-- Otimizações de startup (`vim.loader`, providers) ficam no topo do `init.lua`,
-- para valerem antes do lazy.nvim carregar.

-- Treesitter no Windows compila parsers com o gcc do WinLibs (sem MSVC/cl.exe).
-- Respeita um CC já definido pelo usuário.
if vim.fn.has "win32" == 1 and not vim.env.CC then vim.env.CC = "gcc" end

-- Modo agentic: arquivos alterados por um agente (Claude, opencode…) aparecem
-- sozinhos. `autoread` só recarrega quando o Neovim checa o disco, então
-- checamos ao voltar o foco, trocar de buffer, sair do terminal do agente e
-- quando o cursor fica parado (CursorHold).
vim.opt.autoread = true
vim.api.nvim_create_autocmd({ "FocusGained", "BufEnter", "TermLeave", "CursorHold" }, {
  group = vim.api.nvim_create_augroup("astrograph_autoread", { clear = true }),
  desc = "Recarregar arquivos alterados fora do editor",
  callback = function()
    if vim.fn.mode() ~= "c" and vim.fn.getcmdwintype() == "" then pcall(vim.cmd.checktime) end
  end,
})

-- Scroll linha a linha suave (complementa as animações do Snacks)
vim.opt.smoothscroll = true

-- Sai do modo terminal (ex.: janela do agente) com Esc Esc
vim.keymap.set("t", "<Esc><Esc>", [[<C-\><C-n>]], { desc = "Sair do modo terminal" })

-- :Astrograph atualizar | saude | agentes | configurar | login | atalhos | docs
require("astrograph.commands").setup()

-- Área de transferência assíncrona: copiar não espera o win32yank/pbcopy/xclip
require("astrograph.clipboard").setup()

-- Agente central: trocar o agente no chat (`ga`) troca o de todo o modo agentic
require("astrograph.agents").setup()

-- Modo do agente (permissões), o mesmo em todas as telas: lembra na abertura
-- quando o salvo é "Sem permissões"
require("astrograph.modes").setup()

-- Ações agênticas (corrigir, explicar, revisar, commit), login checado em
-- segundo plano e indicador do agente trabalhando
require("astrograph.agentic").setup()

-- Agente nativo (astrograph-agent, Rust/ACP): segue o agente pelos arquivos,
-- mostra o diff antes de editar e destaca o que mudou depois
require("astrograph.agent").setup()

-- Pré-aquecimento no ocioso: a 1ª busca/explorador tão rápidos quanto os seguintes
require("astrograph.warmup").setup()

-- Treesitter: o instalador pré-compila os parsers; se ainda faltar algum,
-- avisa enquanto compila em vez de deixar o realce sumido sem explicação
require("astrograph.treesitter").setup()

-- Preparo em segundo plano: parsers, ferramentas e git do projeto prontos ao
-- abrir (e ao trocar de pasta); ícone de carregamento no canto inferior direito
require("astrograph.prepare").setup()
