"""The shop's pictures: every piece of the people, rendered on its own.

    FLUNKI_HATS=<hats dir> cargo test --release --lib dump_hats -- --ignored
    blender -b -P tools/icons_blender.py -- <scratch dir with people_shown.png> <hats dir> <out dir>
    python3 -I tools/icons_pack.py <out dir>

Renders assets/people_m.glb and people_f.glb piece by piece (tops with their arms down), each outfit whole
(<m|f>_set_<outfit>.png), and the game's hats (hat_<n>.ply from `scene::dump_hats`), lit like a shop shelf, on a
transparent background, into <out dir>. tools/icons_pack.py puts them into assets/shop<page>.png.
"""
import math
import os
import sys

import bpy
import mathutils

scratch, hats, out = sys.argv[sys.argv.index("--") + 1:]
SIZE = 320
os.makedirs(out, exist_ok=True)
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
for rig in ("m", "f"):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=f"{root}/assets/people_{rig}.glb")
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items] else "BLENDER_EEVEE"
    scene.render.resolution_x = scene.render.resolution_y = SIZE
    scene.render.film_transparent = True
    scene.view_settings.view_transform = "Standard"
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.75, 0.78, 0.85, 1)
    world.node_tree.nodes["Background"].inputs[1].default_value = 0.9
    scene.world = world
    # one material for all: the palette in the shop's colours
    img = bpy.data.images.load(f"{scratch}/people_shown.png")
    mat = bpy.data.materials.new("shown")
    mat.use_nodes = True
    nt = mat.node_tree
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Closest"
    bsdf = nt.nodes["Principled BSDF"]
    bsdf.inputs["Roughness"].default_value = 0.7
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    arm = next(o for o in scene.objects if o.type == "ARMATURE")
    meshes = [o for o in scene.objects if o.type == "MESH"]
    for o in meshes:
        o.data.materials.clear()
        o.data.materials.append(mat)
    # arms down (the rig is in a T-pose): rotate the upper arms about the forward axis
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="POSE")
    for side, sgn in (("l", 1), ("r", -1)):
        pb = arm.pose.bones.get(f"upperarm_{side}")
        if pb:
            head = pb.head.copy()
            rot = mathutils.Matrix.Rotation(sgn * math.radians(70), 4, "Y")
            pb.matrix = mathutils.Matrix.Translation(head) @ rot @ mathutils.Matrix.Translation(-head) @ pb.matrix
    bpy.ops.object.mode_set(mode="OBJECT")
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam.data.type = "ORTHO"
    key = bpy.data.objects.new("key", bpy.data.lights.new("key", "SUN"))
    key.data.energy = 3.2
    key.rotation_euler = (math.radians(50), 0, math.radians(35))
    scene.collection.objects.link(key)
    rim = bpy.data.objects.new("rim", bpy.data.lights.new("rim", "SUN"))
    rim.data.energy = 2.0
    rim.rotation_euler = (math.radians(-60), 0, math.radians(200))
    scene.collection.objects.link(rim)
    dg = bpy.context.evaluated_depsgraph_get()

    def shoot(objs, path, d=(0.55, -1.0, 0.35)):
        for other in [o for o in scene.objects if o.type == "MESH"]:
            other.hide_render = other not in objs
        pts = []
        for o in objs:
            ev = o.evaluated_get(dg)
            pts += [ev.matrix_world @ v.co for v in ev.to_mesh().vertices]
            ev.to_mesh_clear()
        lo = mathutils.Vector([min(p[i] for p in pts) for i in range(3)])
        hi = mathutils.Vector([max(p[i] for p in pts) for i in range(3)])
        c = (lo + hi) / 2
        size = max(hi - lo)
        # 3/4 from the front (glTF's +Z front is Blender's -Y), a little from above
        d = mathutils.Vector(d).normalized()
        cam.location = c + d * (size * 4 + 1)
        cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
        cam.data.ortho_scale = size * 1.12
        cam.data.clip_end = size * 10 + 10
        scene.render.filepath = path
        bpy.ops.render.render(write_still=True)

    for o in meshes:
        shoot([o], f"{out}/{rig}_{o.name}.png")
        print("ICON", rig, o.name)
    # whole outfits: every piece whose name starts with the outfit's
    sets = sorted({o.name.rsplit("_", 1)[0] for o in meshes})
    for st in sets:
        parts = [o for o in meshes if o.name.rsplit("_", 1)[0] == st]
        shoot(parts, f"{out}/{rig}_set_{st}.png", (0.35, -1.0, 0.12))
        print("ICON", rig, "set", st)
    if rig == "f":
        # the hats: the game's own shapes and colours
        hmat = bpy.data.materials.new("hat")
        hmat.use_nodes = True
        ca = hmat.node_tree.nodes.new("ShaderNodeVertexColor")
        hb = hmat.node_tree.nodes["Principled BSDF"]
        hb.inputs["Roughness"].default_value = 0.55
        hmat.node_tree.links.new(ca.outputs["Color"], hb.inputs["Base Color"])
        for h in range(1, 11):
            bpy.ops.wm.ply_import(filepath=f"{hats}/hat_{h}.ply")
            o = bpy.context.selected_objects[0]
            o.data.materials.clear()
            o.data.materials.append(hmat)
            dg = bpy.context.evaluated_depsgraph_get()
            shoot([o], f"{out}/hat_{h}.png", (0.55, -1.0, 0.6))
            o.hide_render = True
            print("ICON hat", h)
