#!/usr/bin/env python3
"""Gera a arte "assada" do dashboard do Astrograph.

Produz `template/lua/astrograph/art.lua` com:
  - o banner `ASTROGRAPH` em 4 larguras (pyfiglet);
  - a lua crescente em 2 tamanhos;
  - as posicoes fixas (seed constante) das estrelas para o canvas grande.

Só quem mexe na arte roda este script. Requer `pyfiglet`:

    pip install pyfiglet
    python tools/gen_art.py
"""

from __future__ import annotations

import io
import os
import random

import pyfiglet

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "template", "lua", "astrograph", "art.lua")

BANNER_FONTS = ["ansi_shadow", "standard", "slant", "mini"]
WORD = "ASTROGRAPH"

# Lua crescente aprovada no mockup (11 linhas x 11 col).
MOON_LARGE = [
    "       \u2588███",
    "    \u2588███",
    "  \u2588███",
    " \u2588███",
    " ████",
    "█████",
    " ████",
    " ████",
    "  \u2588███",
    "    ████",
    "       ████",
]

# Lua compacta (9 linhas) usada em terminais estreitos.
MOON_SMALL = [
    "    ███",
    "  ████",
    "  ███",
    " ████",
    " ████",
    " ████",
    "  ███",
    "  ████",
    "    ███",
]

# Canvas alvo (mesmo das estrelas assadas) para a versao grande.
CANVAS_W = 96
CANVAS_H = 16
MOON_X, MOON_Y = 0, 2
BANNER_X, BANNER_Y = 13, 5
STAR_COUNT = 20
SEED = 42
STARS = "✦✧·✶"


def figlet(font: str, width: int = 200) -> list[str]:
    art = pyfiglet.figlet_format("ASTROGRAPH", font=font, width=width)
    return [line.rstrip() for line in art.rstrip("\n").split("\n") if line.strip()]


def lua_str(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def lua_lines(lines: list[str], indent: str = "        ") -> str:
    return ",\n".join(indent + lua_str(line) for line in lines)


def stars() -> list[tuple[int, int, int]]:
    # usa o banner ansi_shadow para o bounding box
    banner = FIGLET["ansi_shadow"]
    banner_w = max(len(l) for l in banner)
    blocked: set[tuple[int, int]] = set()
    for j in range(MOON_Y, MOON_Y + len(MOON_LARGE)):
        for i in range(MOON_X, MOON_X + max(len(l) for l in MOON_LARGE)):
            blocked.add((j, i))
    for j in range(BANNER_Y, BANNER_Y + len(banner)):
        for i in range(BANNER_X, BANNER_X + banner_w):
            blocked.add((j, i))
    cells = [(y, x) for y in range(CANVAS_H) for x in range(CANVAS_W) if (y, x) not in blocked]
    rng = random.Random(SEED)
    chosen = rng.sample(cells, STAR_COUNT)
    out = []
    for y, x in sorted(chosen):
        phase = rng.randint(1, 3)
        out.append((y, x, phase))
    return out


def main() -> None:
    global FIGLET, BANNER_LINES
    FIGLET = {font: figlet(font) for font in ("ansi_shadow", "standard", "slant", "mini")}
    BANNER_LINES = list(FIGLET.values())

    banners = []
    for font in ("ansi_shadow", "standard", "slant", "mini"):
        lines = FIGLET[font]
        banners.append(
            "    %s = {\n      width = %d,\n      height = %d,\n      lines = {\n%s,\n      },\n    },"
            % (font, max(len(l) for l in lines), len(lines), lua_lines(lines))
        )

    moon = []
    for name, lines in (("large", MOON_LARGE), ("small", MOON_SMALL)):
        moon.append(
            "    %s = {\n      width = %d,\n      height = %d,\n      lines = {\n%s,\n      },\n    },"
            % (name, max(len(l) for l in lines), len(lines), lua_lines(lines))
        )

    star_rows = []
    for y, x, phase in stars():
        star_rows.append("    { y = %d, x = %d, s = %d }," % (y, x, phase))

    body = (
        "-- Arquivo gerado por tools/gen_art.py -- nao edite a mao.\n"
        "-- Regenere com: python tools/gen_art.py\n"
        "\n"
        "return {\n"
        "  starset = { \"✦\", \"✧\", \"·\", \"✶\" },\n"
        "  banners = {\n%s\n  },\n"
        "  moon = {\n%s\n  },\n"
        "  stars = {\n%s\n  },\n"
        "}\n"
        % (
            "\n".join(banners),
            "\n".join(moon),
            "\n".join(star_rows),
        )
    )

    with io.open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(body)
    print("wrote %s" % os.path.normpath(OUT))
    for name in ("ansi_shadow", "standard", "slant", "mini"):
        print("  banner %-12s %dx%d" % (name, max(len(l) for l in FIGLET[name]), len(FIGLET[name])))


if __name__ == "__main__":
    main()
