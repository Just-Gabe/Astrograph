-- This will run last in the setup process.
-- This is just pure lua so anything that doesn't
-- fit in the normal config locations above can go here

-- Otimização geral (performance): `vim.loader` e providers ficam no topo do
-- `init.lua`, para valerem antes do lazy.nvim carregar.

-- Treesitter compila parsers com o gcc do WinLibs (não tem MSVC/cl.exe)
if vim.fn.has "win32" == 1 then vim.env.CC = "gcc" end

-- Recarrega arquivos alterados pelo Claude
vim.opt.autoread = true

-- Scroll linha-a-linha suave (complementa as animações do Snacks)
vim.opt.smoothscroll = true

-- Sai do modo terminal (ex: janela do Claude) com Esc Esc
vim.keymap.set("t", "<Esc><Esc>", [[<C-\><C-n>]], { desc = "Sair do modo terminal" })
