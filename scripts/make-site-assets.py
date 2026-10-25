"""Copies the icon sizes the website uses into site/img and makes the social-preview picture (site/img/og.png, 1200x630).

    python scripts/make-site-assets.py

Run scripts/make-icon.py and scripts/native/shots-site.ps1 first (the icon and the pictures of the notch come from them).
"""
import os
import shutil
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICON = os.path.join(ROOT, 'assets', 'icon')
IMG = os.path.join(ROOT, 'site', 'img')


def font(size, bold=False):
    for name in ('segoeuib.ttf' if bold else 'segoeui.ttf', 'arialbd.ttf' if bold else 'arial.ttf'):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def main():
    os.makedirs(IMG, exist_ok=True)
    for size in (64, 256):
        shutil.copy(os.path.join(ICON, f'blink-{size}.png'), os.path.join(IMG, f'blink-{size}.png'))
    shutil.copy(os.path.join(ICON, 'icon.ico'), os.path.join(IMG, 'favicon.ico'))

    og = Image.new('RGBA', (1200, 630), (11, 13, 20, 255))
    # The two soft lights of the page, blurred, then the picture of the notch on its stage.
    glow = Image.new('RGBA', (1200, 630), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse((-260, -300, 640, 300), fill=(58, 155, 255, 120))
    gd.ellipse((620, -260, 1420, 340), fill=(139, 92, 246, 110))
    og = Image.alpha_composite(og, glow.filter(ImageFilter.GaussianBlur(110)))
    d = ImageDraw.Draw(og)
    icon = Image.open(os.path.join(ICON, 'blink-256.png')).convert('RGBA').resize((104, 104), Image.LANCZOS)
    og.alpha_composite(icon, (72, 48))
    d.text((196, 46), 'Blink', font=font(68, True), fill=(255, 255, 255))
    d.text((198, 122), 'for Windows', font=font(30), fill=(152, 160, 184))
    d.text((72, 196), 'A little notch for your', font=font(50, True), fill=(232, 235, 245))
    d.text((72, 254), 'deadlines, music and files.', font=font(50, True), fill=(232, 235, 245))
    d.text((72, 326), 'Free and open source. No account, no tracking.', font=font(26), fill=(152, 160, 184))
    shot_path = os.path.join(IMG, 'page-deadlines.png')
    if os.path.exists(shot_path):
        stage = Image.new('RGBA', (1056, 250), (0, 0, 0, 0))
        sd = ImageDraw.Draw(stage)
        sd.rounded_rectangle((0, 0, 1056, 300), radius=28, fill=(44, 52, 108, 255))
        shot = Image.open(shot_path).convert('RGBA')
        shot = shot.resize((int(shot.width * 0.52), int(shot.height * 0.52)), Image.LANCZOS)
        stage.alpha_composite(shot, ((1056 - shot.width) // 2, 24))
        og.alpha_composite(stage, (72, 384))
    og.convert('RGB').save(os.path.join(IMG, 'og.png'), optimize=True)
    print('site assets written to', IMG)


if __name__ == '__main__':
    main()
