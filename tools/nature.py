#!/usr/bin/env python3
"""The park's plants and stones from Quaternius' Stylized Nature MegaKit (CC0,
https://quaternius.com/packs/stylizednaturemegakit.html): assets/nature.glb and assets/nature.png.

    python3 -I tools/nature.py <the kit's folder (with glTF/ and Textures/)>

Each model becomes a mesh (node transforms baked in, its base at y = 0) with POSITION, NORMAL, TEXCOORD_0 (as
the kit has them: bark repeats), COLOR_0 (the kit's vertex colour) and TEXCOORD_1 = (texture layer, sway:
how much the wind moves it, 0 at the root). The textures become layers of 512² stacked in nature.png (see
`nature::LAYERS`); the shader cuts out what's under half alpha.
"""
import json
import os
import struct
import sys

import numpy as np
from PIL import Image

MODELS = ["CommonTree_1", "CommonTree_2", "CommonTree_3", "CommonTree_4", "CommonTree_5", "TwistedTree_1", "TwistedTree_3",
          "Bush_Common", "Bush_Common_Flowers", "Grass_Common_Short", "Grass_Common_Tall", "Grass_Wispy_Short", "Grass_Wispy_Tall",
          "Fern_1", "Clover_1", "Clover_2", "Flower_3_Group", "Flower_4_Group", "Plant_1", "Plant_7",
          "Rock_Medium_1", "Rock_Medium_2", "Rock_Medium_3", "Pebble_Round_1", "Pebble_Round_2", "Pebble_Round_3", "Mushroom_Common"]
# texture → layer; and whether the wind moves what wears it
LAYERS = [("Bark_NormalTree.png", False), ("Bark_TwistedTree.png", False), ("Leaves_NormalTree_C.png", True), ("Leaves_TwistedTree.png", True),
          ("Grass.png", True), ("Leaves.png", True), ("Flowers.png", True), ("Rocks_Diffuse.png", False), ("PathRocks_Diffuse.png", False),
          ("Mushrooms.png", False)]
SIZE = 512
# textures that are white to be tinted: the green they get here (the kit's autumn red isn't the Vondelpark's)
TINT = {"Leaves_TwistedTree.png": (0.42, 0.56, 0.16)}


def acc(j, b, i):
    a = j["accessors"][i]
    v = j["bufferViews"][a["bufferView"]]
    n = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
    dt = {5126: "<f4", 5123: "<u2", 5121: "u1", 5125: "<u4"}[a["componentType"]]
    o = v.get("byteOffset", 0) + a.get("byteOffset", 0)
    out = np.frombuffer(b, dtype=dt, count=a["count"] * n, offset=o).reshape(a["count"], n).astype(np.float64)
    if a.get("normalized"):
        out /= {5121: 255.0, 5123: 65535.0}[a["componentType"]]
    return out


def quat(q):
    x, y, z, w = q
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def local(n):
    m = np.eye(4)
    m[:3, :3] = quat(n.get("rotation", [0, 0, 0, 1])) @ np.diag(n.get("scale", [1, 1, 1]))
    m[:3, 3] = n.get("translation", [0, 0, 0])
    return m


def main(kit):
    bin_ = bytearray()
    views, accs, meshes = [], [], []

    def add(arr, ctype, typ, normalized=False):
        arr = np.ascontiguousarray(arr)
        while len(bin_) % 4:
            bin_.append(0)
        views.append({"buffer": 0, "byteOffset": len(bin_), "byteLength": arr.nbytes})
        bin_.extend(arr.tobytes())
        accs.append({"bufferView": len(views) - 1, "componentType": ctype, "count": arr.shape[0], "type": typ})
        if normalized:
            accs[-1]["normalized"] = True
        return len(accs) - 1
    for name in MODELS:
        path = f"{kit}/glTF/{name}.gltf"
        j = json.load(open(path))
        b = open(os.path.join(os.path.dirname(path), j["buffers"][0]["uri"]), "rb").read()
        par = {c: i for i, n in enumerate(j["nodes"]) for c in n.get("children", [])}

        def glob(i):
            return (glob(par[i]) if i in par else np.eye(4)) @ local(j["nodes"][i])
        P, N, U, C, L, I = [], [], [], [], [], []
        base = 0
        for ni, node in enumerate(j["nodes"]):
            if "mesh" not in node:
                continue
            m = glob(ni)
            for p in j["meshes"][node["mesh"]]["primitives"]:
                at = p["attributes"]
                mat = j["materials"][p["material"]]
                tex = mat.get("pbrMetallicRoughness", {}).get("baseColorTexture", {}).get("index")
                uri = j["images"][j["textures"][tex]["source"]]["uri"].replace("Leaves_TwistedTree_C", "Leaves_TwistedTree")
                layer = [l[0] for l in LAYERS].index(uri)
                pos = acc(j, b, at["POSITION"]) @ m[:3, :3].T + m[:3, 3]
                nrm = acc(j, b, at["NORMAL"]) @ np.linalg.inv(m[:3, :3])
                nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-9)
                col = acc(j, b, at["COLOR_0"])[:, :3] if "COLOR_0" in at else np.ones((len(pos), 3))
                P.append(pos), N.append(nrm), U.append(acc(j, b, at["TEXCOORD_0"])), C.append(col)
                L.append(np.full(len(pos), layer))
                I.append(acc(j, b, p["indices"]).astype(np.int64).flatten() + base)
                base += len(pos)
        pos = np.concatenate(pos_ := P)
        lo = pos.min(0)
        pos[:, 1] -= lo[1]
        h = max(pos[:, 1].max(), 1e-3)
        layers = np.concatenate(L)
        # the wind: nothing at the root, more the higher up; only leaves, grass and flowers
        sway = np.where(np.array([LAYERS[int(l)][1] for l in layers]), (pos[:, 1] / h) ** 1.5, 0.0)
        attrs = {"POSITION": add(pos.astype(np.float32), 5126, "VEC3"),
                 "NORMAL": add(np.round(np.concatenate(N) * 127).astype(np.int8), 5120, "VEC3", True),
                 "TEXCOORD_0": add(np.concatenate(U).astype(np.float32), 5126, "VEC2"),
                 "COLOR_0": add(np.round(np.clip(np.concatenate(C), 0, 1) * 255).astype(np.uint8), 5121, "VEC3", True),
                 "TEXCOORD_1": add(np.column_stack([layers, sway]).astype(np.float32), 5126, "VEC2")}
        meshes.append({"name": name, "primitives": [{"attributes": attrs, "indices": add(np.concatenate(I).astype(np.uint32), 5125, "SCALAR")}]})
        print(name, base, "vertices", sum(len(x) for x in I) // 3, "triangles", f"{h:.2f} m high, {np.ptp(pos[:, 0]):.2f} wide")
    nodes = [{"name": m["name"], "mesh": i} for i, m in enumerate(meshes)]
    doc = {"asset": {"version": "2.0", "generator": "tools/nature.py", "copyright": "Quaternius, CC0"}, "scene": 0,
           "scenes": [{"nodes": list(range(len(nodes)))}], "nodes": nodes, "meshes": meshes,
           "buffers": [{"byteLength": 0}], "bufferViews": views, "accessors": accs}
    while len(bin_) % 4:
        bin_.append(0)
    doc["buffers"][0]["byteLength"] = len(bin_)
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    with open("assets/nature.glb", "wb") as f:
        f.write(struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(js) + 8 + len(bin_)))
        f.write(struct.pack("<II", len(js), 0x4E4F534A) + js)
        f.write(struct.pack("<II", len(bin_), 0x004E4942) + bytes(bin_))
    print("assets/nature.glb", os.path.getsize("assets/nature.glb"), "bytes")
    strip = Image.new("RGBA", (SIZE, SIZE * len(LAYERS)))
    for k, (t, _) in enumerate(LAYERS):
        img = Image.open(f"{kit}/Textures/{t}").convert("RGBA").resize((SIZE, SIZE), Image.LANCZOS)
        if t in TINT:
            a = np.asarray(img).astype(np.float64)
            a[..., :3] *= np.array(TINT[t])
            img = Image.fromarray(a.astype(np.uint8), "RGBA")
        strip.paste(img, (0, k * SIZE))
    strip.save("assets/nature.png", optimize=True)
    print("assets/nature.png", os.path.getsize("assets/nature.png"), "bytes")


if __name__ == "__main__":
    main(sys.argv[1])
