#!/usr/bin/env python3
"""Packs the pictures from tools/icons_blender.py into assets/shop<page>.png: 256 px each, 8 × 8 to a page
(2048², what WebGL2 can hold), palette-quantized. Picture k is, in this order: the clothes in the order the
game reads the meshes (assets/people_m.glb's, then people_f.glb's: item 32 + k), the hats 1–10, then the
costumes in `proto::COSTUMES` order. See `shop::icon_of`.

    python3 -I tools/icons_pack.py <dir with the pictures>
"""
import glob
import json
import os
import re
import struct
import sys

from PIL import Image

ICON, SIDE = 256, 8
names = []
for rig in ("m", "f"):
    b = open(f"assets/people_{rig}.glb", "rb").read()
    jl = struct.unpack("<I", b[12:16])[0]
    names += [f"{rig}_{m['name']}" for m in json.loads(b[20:20 + jl])["meshes"]]
names += [f"hat_{h}" for h in range(1, 11)]
costumes = re.findall(r'\("([mf])/(\w+)", "[^"]+"\)', open("core/src/proto.rs").read().split("pub const COSTUMES")[1].split("];")[0])
names += [f"{rig}_set_{st}" for rig, st in costumes]
for old in glob.glob("assets/shop*.png"):
    os.remove(old)
per = SIDE * SIDE
for page in range((len(names) + per - 1) // per):
    sheet = Image.new("RGBA", (ICON * SIDE, ICON * SIDE), (0, 0, 0, 0))
    for k, n in enumerate(names[page * per:(page + 1) * per]):
        img = Image.open(f"{sys.argv[1]}/{n}.png").convert("RGBA").resize((ICON, ICON), Image.LANCZOS)
        sheet.paste(img, ((k % SIDE) * ICON, (k // SIDE) * ICON))
    sheet = sheet.quantize(colors=256, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE)
    sheet.save(f"assets/shop{page}.png", optimize=True)
    print(f"assets/shop{page}.png", os.path.getsize(f"assets/shop{page}.png"), "bytes")
print(len(names), "pictures")
