"""Gera o icone do Astrograph (.ico, varios tamanhos) para o executavel da UI.

Mesmo desenho do icone da janela (`make_icon` em prototype/astrograph-ui/src/main.rs):
quadrado arredondado grafite com a estrela em degrade azul -> ciano.

Uso: python tools/gen_icon.py
"""

import os

from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), "..", "prototype", "astrograph-ui", "assets", "astrograph.ico")


def star(n):
    bg = (0x1F, 0x1F, 0x24)
    top = (0x7D, 0xD3, 0xFC)
    bottom = (0x40, 0x9C, 0xFF)
    radius = n * 0.22
    ss = 4  # supersampling para bordas suaves
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    px = img.load()
    half = n / 2
    for y in range(n):
        for x in range(n):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sy in range(ss):
                for sx in range(ss):
                    fx = x + (sx + 0.5) / ss
                    fy = y + (sy + 0.5) / ss
                    dx = max(abs(fx - half) - (half - radius), 0.0)
                    dy = max(abs(fy - half) - (half - radius), 0.0)
                    if (dx * dx + dy * dy) ** 0.5 > radius:
                        continue
                    u = (fx - half) / half
                    v = (fy - half) / half
                    shape = abs(u) ** 0.55 + abs(v) ** 0.55
                    a = min(max((1.05 - shape) * 6.0, 0.0), 1.0)
                    t = min(max((v + 1) / 2, 0.0), 1.0)
                    col = [top[i] * (1 - t) + bottom[i] * t for i in range(3)]
                    rgb = [bg[i] * (1 - a) + col[i] * a for i in range(3)]
                    acc[0] += rgb[0]
                    acc[1] += rgb[1]
                    acc[2] += rgb[2]
                    acc[3] += 1
            if acc[3]:
                k = ss * ss
                px[x, y] = (int(acc[0] / acc[3]), int(acc[1] / acc[3]), int(acc[2] / acc[3]), int(255 * acc[3] / k))
    return img


def main():
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    big = star(256)
    big.save(OUT, sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("icone gerado em", os.path.normpath(OUT))


if __name__ == "__main__":
    main()
