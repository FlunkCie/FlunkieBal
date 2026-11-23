#!/usr/bin/env python3
"""Makes the game's people from Quaternius' Ultimate Modular Men and Ultimate Modular Women packs (CC0,
https://quaternius.com/packs/ultimatemodularcharacters.html, .../ultimatemodularwomen.html), as converted
to libGDX's G3DJ by fbx-conv: assets/people_m.glb, assets/people_f.glb and assets/people.png.

    python3 -I tools/modular.py <men dir with m_*.json> <women dir with *.json> <scratch dir>

and then the shop's pictures (assets/shop.png) with tools/icons_blender.py, see there. The scratch dir gets
people_shown.png: the palette with the tintable swatches in the colours the pictures show them in.

Every character comes as four pieces (head with its hair, body, legs, feet), all skinned to one Mixamo rig per
sex, so any piece goes with any other. Here: the rig is cut down to the bones the game moves (fingers into the
hand, toes into the foot) and named as `model.rs` expects, turned Y-up, and each material colour becomes a
swatch in a small palette texture. Swatches that take one of the person's colours (`SkinDraw.colors`: 0 the
team on the body piece's main colour, 2 the shorts colour on the legs' main colour, 1 skin, 5 hair) hold a
neutral shade (0.5 = as bright as the material was) with alpha 1; the slot goes in COLOR_0.x.
"""
import glob
import json
import os
import struct
import sys

import numpy as np
from PIL import Image

# Mixamo → the names `model.rs` uses; the rest go to their nearest kept ancestor
NAMES = {"Hips": "pelvis", "Spine": "spine_01", "Spine1": "spine_02", "Spine2": "spine_03", "Neck": "neck_01", "Head": "Head",
         "LeftShoulder": "clavicle_l", "LeftArm": "upperarm_l", "LeftForeArm": "lowerarm_l", "LeftHand": "hand_l",
         "LeftUpLeg": "thigh_l", "LeftLeg": "calf_l", "LeftFoot": "foot_l",
         "RightShoulder": "clavicle_r", "RightArm": "upperarm_r", "RightForeArm": "lowerarm_r", "RightHand": "hand_r",
         "RightUpLeg": "thigh_r", "RightLeg": "calf_r", "RightFoot": "foot_r"}
KEEP = ["root"] + list(NAMES.values())
# Blender's Z-up (front -Y) → Y-up (front +Z)
C = np.array([[1, 0, 0, 0], [0, 0, 1, 0], [0, -1, 0, 0], [0, 0, 0, 1]], np.float64)
# pieces named in a way of their own: the backpack goes on your back, the farmer's trousers are trousers
RENAME = {"Backpack": "Backpack_Back", "Farmer_Pants": "Farmer_Legs", "Formad_Head": "Formal_Head"}
# the materials that are hair, per head (otherwise: those called Hair…); what stays is face, brows and headgear
HAIR = {("m", "Punk_Head"): ["Red.001", "Red_Dark.002"], ("m", "Farmer_Head"): [], ("m", "Worker_Head"): [],
        ("f", "Formad_Head"): ["Red"], ("f", "Medieval_Head"): ["White"], ("f", "Worker_Head"): ["DarkBrown"],
        ("f", "Punk_Head"): ["Hair_Brown", "Pink"]}
SW = 8  # swatch size in texels
PAL = 256


def quat(q):
    x, y, z, w = q
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def trs(b):
    m = np.eye(4)
    m[:3, :3] = quat(b.get("rotation", [0, 0, 0, 1])) @ np.diag(b.get("scale", [1, 1, 1])[:3])
    m[:3, 3] = b.get("translation", [0, 0, 0])[:3]
    return m


def unscaled(m):
    m = m.copy()
    m[:3, :3] /= np.linalg.norm(m[:3, :3], axis=0)
    return m


def short(n):
    return n.split(":")[-1]


def lum(c):
    return float(np.dot(c, [0.2126, 0.7152, 0.0722]))


def load_pieces(files, parents):
    """Every piece in these G3DJ files: name → list of (material colour, positions, normals, joints, weights, triangles)."""
    pieces, binds = {}, {}
    for f in files:
        j = json.load(open(f))
        mats = {m["id"]: (m["id"], np.array(m["diffuse"][:3])) for m in j["materials"]}
        parts = {}
        for mesh in j["meshes"]:
            attrs = mesh["attributes"]
            size = {"POSITION": 3, "NORMAL": 3, "TEXCOORD0": 2, "COLOR": 4, "COLORPACKED": 1, "TANGENT": 3, "BINORMAL": 3}
            off, at = {}, 0
            for a in attrs:
                off[a] = at
                at += size.get(a, 2)
            verts = np.array(mesh["vertices"], np.float64).reshape(-1, at)
            # position, normal, then the four (bone, weight) pairs
            bw = [off[f"BLENDWEIGHT{k}"] for k in range(4) if f"BLENDWEIGHT{k}" in off]
            cols = list(range(0, 6)) + [c + d for c in bw for d in (0, 1)]
            verts = verts[:, cols]
            if len(bw) < 4:
                verts = np.hstack([verts, np.zeros((len(verts), 8 - 2 * len(bw)))])
            for p in mesh["parts"]:
                parts[p["id"]] = (verts, np.array(p["indices"], np.int64).reshape(-1, 3))

        def walk(n, par=None):
            for c in n.get("children", []):
                parents.setdefault(short(c["id"]), short(n["id"]))
                walk(c, n)
        for n in j["nodes"]:
            walk(n)
        for n in j["nodes"]:
            if "parts" not in n:
                continue
            prims = []
            for p in n["parts"]:
                bones = [short(b["node"]) for b in p["bones"]]
                B = [C @ unscaled(trs(b)) for b in p["bones"]]
                for name, m in zip(bones, B):
                    binds.setdefault(name, m)
                verts, tri = parts[p["meshpartid"]]
                used = np.unique(tri)
                remap = -np.ones(len(verts), np.int64)
                remap[used] = np.arange(len(used))
                v = verts[used]
                pos = v[:, 0:3] @ C[:3, :3].T
                nrm = v[:, 3:6] @ C[:3, :3].T
                bw = v[:, 6:14].reshape(-1, 4, 2)
                jj = np.zeros((len(v), 4), np.int64)
                ww = np.zeros((len(v), 4))
                for k in range(4):
                    for vi in range(len(v)):
                        b, w = int(bw[vi, k, 0]), bw[vi, k, 1]
                        if w <= 0:
                            continue
                        name = bones[b]
                        while name not in NAMES and name in parents:
                            name = parents[name]
                        tgt = KEEP.index(NAMES.get(name, "Hips") if name in NAMES else "pelvis")
                        # the same bone twice: add up
                        slot = next((s for s in range(4) if ww[vi, s] > 0 and jj[vi, s] == tgt), None)
                        if slot is None:
                            slot = next(s for s in range(4) if ww[vi, s] == 0)
                        jj[vi, slot] = tgt
                        ww[vi, slot] += w
                ww /= np.maximum(ww.sum(1, keepdims=True), 1e-9)
                # the vertices are where the bones put them at bind time: the mesh space is the rig's
                prims.append((mats[p["materialid"]], pos, nrm, jj, ww, remap[tri]))
            pieces[n["id"]] = prims
    return pieces, binds


def smooth(pos, jj, ww, tri, crease=0.5):
    """Smooth normals (faces meeting at more than ~60° keep their edge) on welded vertices."""
    fn = np.cross(pos[tri[:, 1]] - pos[tri[:, 0]], pos[tri[:, 2]] - pos[tri[:, 0]])
    unit = fn / np.maximum(np.linalg.norm(fn, axis=1, keepdims=True), 1e-12)
    key = {}
    for v, p in enumerate(np.round(pos, 5)):
        key.setdefault(tuple(p), []).append(v)
    corners = {}  # position → [(face, corner)]
    for f, t in enumerate(tri):
        for c in range(3):
            corners.setdefault(tuple(np.round(pos[t[c]], 5)), []).append(f)
    out_p, out_n, out_j, out_w, out_t = [], [], [], [], []
    seen = {}
    for f, t in enumerate(tri):
        face = []
        for c in range(3):
            v = t[c]
            k = tuple(np.round(pos[v], 5))
            fs = [g for g in corners[k] if unit[g] @ unit[f] > crease]
            n = fn[fs].sum(0)
            n /= max(np.linalg.norm(n), 1e-12)
            nk = (k, tuple(np.round(n, 3)))
            if nk not in seen:
                seen[nk] = len(out_p)
                out_p.append(pos[v]), out_n.append(n), out_j.append(jj[v]), out_w.append(ww[v])
            face.append(seen[nk])
        out_t.append(face)
    return np.array(out_p), np.array(out_n), np.array(out_j), np.array(out_w), np.array(out_t, np.int64)


def bytes_weights(w):
    """Weights as 255ths that still add up to 255."""
    q = np.round(w * 255).astype(np.int64)
    q[:, 0] += 255 - q.sum(1)
    return np.clip(q, 0, 255).astype(np.uint8)


def piece_kind(name):
    if name == "Farmer_Pants":
        return "Legs"
    for k in ("Hair", "Head", "Body", "Legs", "Feet"):
        if name.endswith("_" + k) or name.endswith(k):
            return k
    return "Feet"  # the women's files call the feet by the character's name


def main(men_dir, women_dir, shown_dir):
    sets = {"m": sorted(glob.glob(f"{men_dir}/*.json")), "f": sorted(glob.glob(f"{women_dir}/*.json"))}
    swatches = []  # (rgb linear, alpha, colour for the shop's pictures)
    # what the tintable swatches look like in the shop's pictures (tools/icons_blender.py): sRGB
    SHOWN = {0: (0.18, 0.53, 0.72), 1: (0.79, 0.56, 0.4), 2: (0.23, 0.27, 0.35), 5: (0.36, 0.22, 0.12)}

    def swatch(rgb, a, slot=255):
        shown = np.array(SHOWN[slot]) ** 2.2 if slot in SHOWN else np.clip(rgb, 0, 1)
        swatches.append((np.clip(rgb, 0, 1), a, shown))
        i = len(swatches) - 1
        x, y = (i % (PAL // SW)) * SW + SW / 2, (i // (PAL // SW)) * SW + SW / 2
        return (x / PAL, y / PAL)
    for rig, files in sets.items():
        parents = {}
        pieces, binds = load_pieces(files, parents)
        # the rig: each kept bone where its bind matrix says, under the nearest kept ancestor
        rev = {v: k for k, v in NAMES.items()}
        G = {"root": np.eye(4)}
        for k in KEEP[1:]:
            G[k] = binds[rev[k]]

        def kept_parent(k):
            if k == "root":
                return None
            p = parents.get(rev[k])
            while p is not None and p not in NAMES:
                p = parents.get(p)
            return NAMES[p] if p else "root"
        nodes = []
        for k in KEEP:
            p = kept_parent(k)
            m = (np.linalg.inv(G[p]) if p else np.eye(4)) @ G[k]
            r3 = m[:3, :3]
            w = np.sqrt(max(1e-12, 1 + r3[0, 0] + r3[1, 1] + r3[2, 2])) / 2
            q = [(r3[2, 1] - r3[1, 2]) / (4 * w), (r3[0, 2] - r3[2, 0]) / (4 * w), (r3[1, 0] - r3[0, 1]) / (4 * w), w]
            nodes.append({"name": k, "translation": m[:3, 3].tolist(), "rotation": q})
        for i, k in enumerate(KEEP):
            kids = [KEEP.index(c) for c in KEEP if kept_parent(c) == k]
            if kids:
                nodes[i]["children"] = kids
        bin_ = bytearray()
        views, accs = [], []

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
        ibm = np.stack([np.linalg.inv(G[k]).T for k in KEEP]).astype(np.float32).reshape(-1, 16)
        skin = {"joints": list(range(len(KEEP))), "inverseBindMatrices": add(ibm, 5126, "MAT4")}
        # hair is a piece of its own (any hairstyle on any head); the head keeps face, brows and headgear
        for name in [n for n in pieces if piece_kind(n) == "Head"]:
            is_hair = (lambda m: m in HAIR[(rig, name)]) if (rig, name) in HAIR else (lambda m: m.lower().startswith("hair"))
            hair = [p for p in pieces[name] if is_hair(p[0][0])]
            if hair:
                pieces[RENAME.get(name, name).replace("_Head", "_Hair")] = hair
                pieces[name] = [p for p in pieces[name] if not is_hair(p[0][0])]
        meshes = []
        for name, prims in sorted(pieces.items()):
            kind = piece_kind(name)
            # the cloth that takes a colour: the body's and the legs' largest non-skin material
            area = {}
            for (mid, col), pos, _, _, _, tri in prims:
                a = np.linalg.norm(np.cross(pos[tri[:, 1]] - pos[tri[:, 0]], pos[tri[:, 2]] - pos[tri[:, 0]]), axis=1).sum()
                area[mid] = area.get(mid, 0) + a
            cloth = [m for m in sorted(area, key=area.get, reverse=True) if not m.lower().startswith("skin")]
            dyed = cloth[0] if cloth and kind in ("Body", "Legs") else None
            P, Nn, U, J, W, S, I = [], [], [], [], [], [], []
            base = 0
            for (mid, col), pos, nrm, jj, ww, tri in prims:
                pos, nrm, jj, ww, tri = smooth(pos, jj, ww, tri)
                low = mid.lower()
                slot = 1 if low.startswith("skin") else 5 if (low.startswith("hair") or low.startswith("eyebrow")) else (0 if kind == "Body" else 2) if mid == dyed else 255
                uv = swatch(np.full(3, 0.5), 1.0, slot) if slot != 255 else swatch(col, 0.0)
                P.append(pos), Nn.append(nrm / np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-9))
                U.append(np.tile(uv, (len(pos), 1))), J.append(jj), W.append(ww), S.append(np.full((len(pos), 4), [slot, 0, 0, 0]))
                I.append(tri + base)
                base += len(pos)
            attrs = {"POSITION": add(np.concatenate(P).astype(np.float32), 5126, "VEC3"),
                     "NORMAL": add(np.round(np.concatenate(Nn) * 127).astype(np.int8), 5120, "VEC3", True),
                     "TEXCOORD_0": add(np.round(np.concatenate(U) * 65535).astype(np.uint16), 5123, "VEC2", True),
                     "JOINTS_0": add(np.concatenate(J).astype(np.uint8), 5121, "VEC4"),
                     "WEIGHTS_0": add(bytes_weights(np.concatenate(W)), 5121, "VEC4", True),
                     "COLOR_0": add(np.concatenate(S).astype(np.uint8), 5121, "VEC4")}
            out_name = RENAME.get(name, name if kind != "Feet" or name.endswith("Feet") else name + "_Feet")
            meshes.append({"name": out_name, "primitives": [{"attributes": attrs, "indices": add(np.concatenate(I).astype(np.uint16).flatten(), 5123, "SCALAR")}]})
            print(rig, out_name, base, "vertices", sum(len(x) for x in I), "triangles", "dyed:", dyed)
        # a node per mesh, after the bones (the game reads only the bones)
        first = len(nodes)
        nodes += [{"name": m["name"], "mesh": i, "skin": 0} for i, m in enumerate(meshes)]
        doc = {"asset": {"version": "2.0", "generator": "tools/modular.py", "copyright": "Quaternius, CC0"},
               "scene": 0, "scenes": [{"nodes": [0] + list(range(first, len(nodes)))}], "nodes": nodes, "skins": [skin], "meshes": meshes,
               "buffers": [{"byteLength": 0}], "bufferViews": views, "accessors": accs}
        while len(bin_) % 4:
            bin_.append(0)
        doc["buffers"][0]["byteLength"] = len(bin_)
        js = json.dumps(doc, separators=(",", ":")).encode()
        js += b" " * (-len(js) % 4)
        out = f"assets/people_{rig}.glb"
        with open(out, "wb") as f:
            f.write(struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(js) + 8 + len(bin_)))
            f.write(struct.pack("<II", len(js), 0x4E4F534A) + js)
            f.write(struct.pack("<II", len(bin_), 0x004E4942) + bytes(bin_))
        print(out, os.path.getsize(out), "bytes")
    pal = np.zeros((PAL, PAL, 4))
    shown = np.zeros((PAL, PAL, 4))
    for i, (rgb, a, sh) in enumerate(swatches):
        x, y = (i % (PAL // SW)) * SW, (i // (PAL // SW)) * SW
        pal[y:y + SW, x:x + SW, :3] = rgb ** (1 / 2.2)
        pal[y:y + SW, x:x + SW, 3] = a
        shown[y:y + SW, x:x + SW, :3] = sh ** (1 / 2.2)
        shown[y:y + SW, x:x + SW, 3] = 1
    assert len(swatches) <= (PAL // SW) ** 2, "palette full"
    Image.fromarray((pal * 255 + 0.5).astype(np.uint8), "RGBA").save("assets/people.png", optimize=True)
    # for tools/icons_blender.py
    Image.fromarray((shown * 255 + 0.5).astype(np.uint8), "RGBA").save(os.path.join(shown_dir, "people_shown.png"))
    print("assets/people.png", len(swatches), "swatches")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3])
