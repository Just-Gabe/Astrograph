"""Gera prints (PNG) do Astrograph direto da grade de UI do Neovim.

Sobe um `nvim --embed`, se conecta como uma UI (msgpack-rpc, `ext_linegrid`),
executa uma sequência de passos e desenha a tela com a JetBrainsMono Nerd Font.
Não depende de terminal nem de captura de tela do sistema: o resultado é
nítido e reprodutível.

Uso:
    python tools/screenshot.py --out docs/img/dashboard.png --step "wait:1500"
    python tools/screenshot.py --out docs/img/editor.png \\
        --file template/lua/astrograph/agents.lua --step "wait:2500" --step "keys:12G"

Passos (`--step`, em ordem):
    wait:<ms>     espera (deixa plugins/animações terminarem)
    keys:<teclas> envia teclas (sintaxe do nvim_input, ex.: "<Space>", "<Esc>")
    cmd:<ex>      roda um comando Ex
    lua:<código>  roda Lua

Requisitos: `pip install msgpack pillow fonttools` e a JetBrainsMono Nerd Font.
O perfil do Neovim é o padrão; use NVIM_APPNAME para apontar outro.
"""

import argparse
import os
import subprocess
import sys
import threading
import time

import msgpack
from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFont


def find_font(name):
    dirs = [
        os.path.join(os.environ.get("LOCALAPPDATA", ""), "Microsoft", "Windows", "Fonts"),
        r"C:\Windows\Fonts",
        os.path.expanduser("~/.local/share/fonts"),
        os.path.expanduser("~/Library/Fonts"),
        "/usr/share/fonts",
    ]
    for d in dirs:
        for root, _, files in os.walk(d):
            if name in files:
                return os.path.join(root, name)
    return None


class Nvim:
    """Cliente msgpack-rpc mínimo para um `nvim --embed`."""

    def __init__(self, args, cwd):
        self.proc = subprocess.Popen(
            args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=cwd, env=os.environ.copy()
        )
        self.packer = msgpack.Packer()
        self.unpacker = msgpack.Unpacker(raw=False, strict_map_key=False)
        self.msgid = 0
        self.responses = {}
        self.lock = threading.Condition()
        self.on_redraw = None
        threading.Thread(target=self._reader, daemon=True).start()

    def _reader(self):
        while True:
            data = self.proc.stdout.read1(65536)
            if not data:
                return
            self.unpacker.feed(data)
            for msg in self.unpacker:
                if msg[0] == 1:  # resposta
                    with self.lock:
                        self.responses[msg[1]] = (msg[2], msg[3])
                        self.lock.notify_all()
                elif msg[0] == 2 and msg[1] == "redraw" and self.on_redraw:
                    with self.lock:
                        self.on_redraw(msg[2])
                elif msg[0] == 0:  # requisição do nvim: responde nil
                    self.proc.stdin.write(self.packer.pack([1, msg[1], None, None]))
                    self.proc.stdin.flush()

    def request(self, method, *args):
        with self.lock:
            self.msgid += 1
            mid = self.msgid
        self.proc.stdin.write(self.packer.pack([0, mid, method, list(args)]))
        self.proc.stdin.flush()
        with self.lock:
            while mid not in self.responses:
                self.lock.wait(30)
            err, res = self.responses.pop(mid)
        if err:
            raise RuntimeError(f"{method}: {err}")
        return res


class Grid:
    """Estado da grade 1 a partir dos eventos `redraw` (ext_linegrid)."""

    def __init__(self, cols, rows):
        self.cols, self.rows = cols, rows
        self.cells = [[(" ", 0) for _ in range(cols)] for _ in range(rows)]
        self.hl = {0: {}}
        self.fg, self.bg, self.sp = 0xC0C0C0, 0x1A1D23, 0xFF0000

    def handle(self, events):
        for ev in events:
            name, calls = ev[0], ev[1:]
            for args in calls:
                fn = getattr(self, "ev_" + name, None)
                if fn:
                    fn(*args)

    def ev_grid_resize(self, grid, cols, rows):
        if grid == 1:
            self.cols, self.rows = cols, rows
            self.cells = [[(" ", 0) for _ in range(cols)] for _ in range(rows)]

    def ev_default_colors_set(self, fg, bg, sp, *_):
        if fg >= 0:
            self.fg = fg
        if bg >= 0:
            self.bg = bg
        if sp >= 0:
            self.sp = sp

    def ev_hl_attr_define(self, hid, rgb, *_):
        self.hl[hid] = rgb

    def ev_grid_clear(self, grid):
        if grid == 1:
            self.cells = [[(" ", 0) for _ in range(self.cols)] for _ in range(self.rows)]

    def ev_grid_line(self, grid, row, col, cells, *_):
        if grid != 1 or row >= self.rows:
            return
        hl = 0
        for cell in cells:
            text = cell[0]
            if len(cell) > 1:
                hl = cell[1]
            rep = cell[2] if len(cell) > 2 else 1
            for _ in range(rep):
                if col < self.cols:
                    self.cells[row][col] = (text, hl)
                col += 1

    def ev_grid_scroll(self, grid, top, bot, left, right, rows, _cols):
        if grid != 1:
            return
        if rows > 0:
            for r in range(top, bot - rows):
                self.cells[r][left:right] = self.cells[r + rows][left:right]
        else:
            for r in range(bot - 1, top - rows - 1, -1):
                self.cells[r][left:right] = self.cells[r + rows][left:right]


# blocos sólidos: (x0, y0, x1, y1) em frações da célula
BLOCKS = {
    "█": (0, 0, 1, 1),
    "▀": (0, 0, 1, 0.5),
    "▄": (0, 0.5, 1, 1),
    "▌": (0, 0, 0.5, 1),
    "▐": (0.5, 0, 1, 1),
}


class Renderer:
    def __init__(self, size):
        reg = find_font("JetBrainsMonoNerdFont-Regular.ttf")
        if not reg:
            sys.exit("JetBrainsMono Nerd Font não encontrada (rode o instalador do Astrograph)")
        bold = find_font("JetBrainsMonoNerdFont-Bold.ttf") or reg
        italic = find_font("JetBrainsMonoNerdFont-Italic.ttf") or reg
        self.fonts = {
            (False, False): ImageFont.truetype(reg, size),
            (True, False): ImageFont.truetype(bold, size),
            (False, True): ImageFont.truetype(italic, size),
            (True, True): ImageFont.truetype(bold, size),
        }
        self.cmap = TTFont(reg, fontNumber=0).getBestCmap()
        fb = [p for p in (find_font("seguisym.ttf"), find_font("seguiemj.ttf"), find_font("DejaVuSans.ttf")) if p]
        self.fallbacks = [(ImageFont.truetype(p, size), TTFont(p).getBestCmap()) for p in fb]
        asc, desc = self.fonts[(False, False)].getmetrics()
        # avanço exato (float) e altura = ascent + descent: os blocos `█` e as
        # linhas de caixa do banner se encostam sem frestas
        self.cw = self.fonts[(False, False)].getlength("M")
        self.ch = asc + desc
        self.base = 0

    def font_for(self, ch, bold, italic):
        cp = ord(ch[0])
        if cp in self.cmap or cp < 128:
            return self.fonts[(bold, italic)]
        for f, cmap in self.fallbacks:
            if cp in cmap:
                return f
        return self.fonts[(bold, italic)]

    def render(self, grid, out, title):
        pad, bar = 28, 38
        w, h = round(grid.cols * self.cw), grid.rows * self.ch
        W, H = w + pad * 2, h + pad * 2 + bar
        img = Image.new("RGB", (W + 48, H + 48), (14, 15, 20))
        frame = Image.new("RGB", (W, H), self.rgb(grid.bg))
        d = ImageDraw.Draw(frame)
        d.rectangle([0, 0, W, bar], fill=self.shade(grid.bg, 0.82))
        for i, c in enumerate([(255, 95, 87), (254, 188, 46), (40, 200, 64)]):
            d.ellipse([18 + i * 22, bar / 2 - 6, 30 + i * 22, bar / 2 + 6], fill=c)
        tf = self.fonts[(False, False)]
        d.text((W / 2 - tf.getlength(title) / 2, bar / 2 - self.ch / 2 + 2), title, font=tf, fill=self.rgb(grid.fg))
        ox, oy = pad, pad + bar
        for r, row in enumerate(grid.cells):
            for c, (text, hid) in enumerate(row):
                # seletores de variação (ex.: o U+FE0F depois de um emoji) não têm glifo próprio
                text = text.replace("\ufe0f", "").replace("\ufe0e", "")
                a = grid.hl.get(hid, {})
                fg, bg = a.get("foreground", grid.fg), a.get("background", grid.bg)
                if a.get("reverse"):
                    fg, bg = bg, fg
                x, y = ox + c * self.cw, oy + r * self.ch
                if bg != grid.bg:
                    d.rectangle([x, y, x + self.cw, y + self.ch], fill=self.rgb(bg))
                if text in BLOCKS:
                    # blocos sólidos desenhados como retângulos: nítidos em qualquer fonte
                    x0, y0, x1, y1 = BLOCKS[text]
                    d.rectangle(
                        [x + x0 * self.cw, y + y0 * self.ch, x + x1 * self.cw - 1, y + y1 * self.ch - 1],
                        fill=self.rgb(fg),
                    )
                elif text and text != " ":
                    font = self.font_for(text, bool(a.get("bold")), bool(a.get("italic")))
                    d.text((x, y + self.base), text, font=font, fill=self.rgb(fg))
                if a.get("underline") or a.get("undercurl"):
                    d.line([x, y + self.ch - 2, x + self.cw, y + self.ch - 2], fill=self.rgb(a.get("special", fg)))
        mask = Image.new("L", (W, H), 0)
        ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, H - 1], radius=14, fill=255)
        img.paste(frame, (24, 24), mask)
        img.save(out, optimize=True)

    @staticmethod
    def rgb(v):
        return ((v >> 16) & 255, (v >> 8) & 255, v & 255)

    @classmethod
    def shade(cls, v, f):
        r, g, b = cls.rgb(v)
        return (int(r * f), int(g * f), int(b * f))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True)
    ap.add_argument("--cols", type=int, default=120)
    ap.add_argument("--rows", type=int, default=34)
    ap.add_argument("--font-size", type=int, default=18)
    ap.add_argument("--file", help="arquivo a abrir")
    ap.add_argument("--cwd", default=os.getcwd())
    ap.add_argument("--title", default="nvim — Astrograph")
    ap.add_argument("--step", action="append", default=[])
    ap.add_argument("--nvim", default="nvim")
    args = ap.parse_args()

    nv = Nvim([args.nvim, "--embed"] + ([args.file] if args.file else []), args.cwd)
    grid = Grid(args.cols, args.rows)
    nv.on_redraw = grid.handle
    nv.request("nvim_ui_attach", args.cols, args.rows, {"rgb": True, "ext_linegrid": True})
    for step in args.step:
        kind, _, val = step.partition(":")
        if kind == "wait":
            time.sleep(int(val) / 1000)
        elif kind == "keys":
            nv.request("nvim_input", val)
        elif kind == "cmd":
            nv.request("nvim_command", val)
        elif kind == "lua":
            nv.request("nvim_exec_lua", val, [])
        time.sleep(0.15)
    time.sleep(0.4)
    with nv.lock:
        Renderer(args.font_size).render(grid, args.out, args.title)
    # notificação (sem esperar resposta): o nvim sai antes de responder
    try:
        nv.proc.stdin.write(nv.packer.pack([2, "nvim_command", ["qa!"]]))
        nv.proc.stdin.flush()
        nv.proc.wait(5)
    except Exception:
        nv.proc.kill()
    print("ok", args.out)


if __name__ == "__main__":
    main()
