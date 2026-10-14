-- This will run last in the setup process.
-- This is just pure lua so anything that doesn't
-- fit in the normal config locations above can go here

-- Otimização geral (performance)
--------------------------------
-- Cache de bytecode Lua: startup mais rápido nas execuções seguintes
-- (cross-platform: Windows, Linux e macOS)
pcall(vim.loader.enable)

-- Evita a checagem de providers remotos não usados no startup
vim.g.loaded_perl_provider = 0
vim.g.loaded_ruby_provider = 0

-- Treesitter compila parsers com o gcc do WinLibs (não tem MSVC/cl.exe)
if vim.fn.has "win32" == 1 then vim.env.CC = "gcc" end

-- Recarrega arquivos alterados pelo Claude
vim.opt.autoread = true

-- Sai do modo terminal (ex: janela do Claude) com Esc Esc
vim.keymap.set("t", "<Esc><Esc>", [[<C-\><C-n>]], { desc = "Sair do modo terminal" })
