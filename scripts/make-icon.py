"""Draws Blink's app icon and writes every size it is needed in.

    python scripts/make-icon.py

A rounded square in a blue-to-violet gradient; a black notch hangs from its top edge with two eyes in it, one open and one
winking (the notch blinks); under it, three faint rows, the deadlines it drops down. Drawn large and shrunk, so the edges are smooth.

Writes assets/icon/blink-1024.png (the master), the sizes the programs and the website use, and icon.ico (16 to 256 pixels).
"""
import os
from PIL import Image, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'assets', 'icon')
S = 2048  # drawn at twice the master, shrunk to 1024 (and from there to the rest)


def gradient(size, top_left, bottom_right):
    """A diagonal gradient, as an RGB image."""
    w, h = size
    base = Image.new('RGB', size)
    px = base.load()
    for y in range(h):
        for x in range(w):
            t = (x / (w - 1) * 0.5 + y / (h - 1) * 0.5)
            px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(top_left, bottom_right))
    return base


def draw():
    margin, radius = 112, 430
    body = (margin, margin, S - margin, S - margin)
    mask = Image.new('L', (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle(body, radius=radius, fill=255)

    # The body: blue to violet, with a soft light along the top.
    fill = gradient((S, S), (0x3a, 0x9b, 0xff), (0x8b, 0x5c, 0xf6))
    light = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(light).ellipse((S * 0.1, -S * 0.55, S * 0.9, S * 0.55), fill=(255, 255, 255, 46))
    light = light.filter(ImageFilter.GaussianBlur(60))
    fill = Image.alpha_composite(fill.convert('RGBA'), light)

    img = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    img.paste(fill, (0, 0), mask)
    d = ImageDraw.Draw(img)

    # Three faint rows: what the notch drops down.
    for i, w in enumerate((1000, 1000, 760)):
        top = 900 + i * 230
        d.rounded_rectangle(((S - w) / 2, top, (S + w) / 2, top + 150), radius=75, fill=(255, 255, 255, 52 if i < 2 else 40))
    # The notch: black, flat at the top edge, round at the bottom.
    nw, nh = 1020, 560
    nx0, nx1, ny0, ny1 = (S - nw) / 2, (S + nw) / 2, margin, margin + nh
    notch = Image.new('L', (S, S), 0)
    nd = ImageDraw.Draw(notch)
    nd.rounded_rectangle((nx0, ny0 - 200, nx1, ny1), radius=280, fill=255)  # its top is cut off by the edge below
    edge = Image.new('L', (S, S), 0)
    ImageDraw.Draw(edge).rectangle((0, ny0, S, S), fill=255)
    notch = Image.composite(notch, Image.new('L', (S, S), 0), edge)
    notch = Image.composite(notch, Image.new('L', (S, S), 0), mask)
    black = Image.new('RGBA', (S, S), (6, 8, 16, 255))
    img.paste(black, (0, 0), notch)

    d = ImageDraw.Draw(img)
    cy = margin + 290
    # The open eye, with a spark of light.
    d.ellipse((S / 2 - 270 - 80, cy - 135, S / 2 - 270 + 80, cy + 135), fill=(250, 252, 255, 255))
    d.ellipse((S / 2 - 270 - 10, cy - 100, S / 2 - 270 + 40, cy - 50), fill=(190, 215, 255, 255))
    # The winking eye: a smile-shaped line.
    wx = S / 2 + 270
    d.arc((wx - 120, cy - 120, wx + 120, cy + 90), start=15, end=165, fill=(250, 252, 255, 255), width=46)
    for angle_x in (wx - 112, wx + 112):
        d.ellipse((angle_x - 23, cy + 10, angle_x + 23, cy + 56), fill=(250, 252, 255, 255))
    return img.resize((1024, 1024), Image.LANCZOS)


def main():
    os.makedirs(OUT, exist_ok=True)
    master = draw()
    master.save(os.path.join(OUT, 'blink-1024.png'))
    for size in (512, 256, 128, 64, 48, 32, 24, 16):
        master.resize((size, size), Image.LANCZOS).save(os.path.join(OUT, f'blink-{size}.png'))
    master.save(os.path.join(OUT, 'icon.ico'), sizes=[(s, s) for s in (256, 128, 64, 48, 32, 24, 16)])
    print('icon written to', OUT)


if __name__ == '__main__':
    main()
