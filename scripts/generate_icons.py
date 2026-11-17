#!/usr/bin/env python3
"""Render HShell's vector master into Linux, Windows and preview assets.

Development tool only: requires librsvg's rsvg-convert and Pillow.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
from PIL import Image, ImageDraw, ImageFont

root = Path(__file__).resolve().parent.parent
master = root / 'src/assets/hshell.svg'
icons = root / 'src-tauri/icons'
shutil.copyfile(master, icons / 'source.svg')
with tempfile.TemporaryDirectory(prefix='hshell-icons-') as directory:
    images = {}
    for size in [16, 24, 32, 48, 64, 128, 256, 512]:
        path = Path(directory) / f'{size}.png'
        subprocess.run(['rsvg-convert', '-w', str(size), '-h', str(size), '-o', str(path), str(master)], check=True)
        images[size] = Image.open(path).convert('RGBA')
        name = '128x128@2x.png' if size == 256 else f'{size}x{size}.png'
        images[size].save(icons / name, optimize=True)
    images[256].save(icons / 'icon.ico', format='ICO', sizes=[(size, size) for size in [16, 24, 32, 48, 64, 128, 256]])
    board = Image.new('RGB', (1000, 400), '#17191d')
    board.paste(images[256], (52, 64), images[256])
    draw = ImageDraw.Draw(board)
    font_file = root / 'native-assets/fonts/ibm-plex-sans-medium.ttf'
    draw.text((370, 83), 'HShell', fill='#e7edf5', font=ImageFont.truetype(str(font_file), 64))
    draw.text((374, 171), 'SSH CLIENT', fill='#a2aab7', font=ImageFont.truetype(str(font_file), 18))
    offset = 376
    for size in [16, 24, 32, 48, 64]:
        board.paste(images[size], (offset, 251 + (64-size)//2), images[size])
        offset += size+28
    output = root / 'docs/hshell-icon.png'
    board.save(output, optimize=True)
    print(output)
