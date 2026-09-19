"""离线导出 Blender 场景为 glTF 及其外部贴图。"""

import argparse
import math
import subprocess
import sys
from pathlib import Path


class BlenderSceneExporter:
    def __init__(self, args):
        self.args = args
        self.source = args.source.resolve()
        self.output = args.output.resolve()

    def run(self):
        if not self.args.worker:
            subprocess.run(
                [
                    str(self.args.blender),
                    "--background",
                    "--factory-startup",
                    "--python-exit-code",
                    "1",
                    "--python",
                    str(Path(__file__).resolve()),
                    "--",
                    "--worker",
                    "--source",
                    str(self.source),
                    "--output",
                    str(self.output),
                    "--texture-size",
                    str(self.args.texture_size),
                ],
                check=True,
            )
            return

        import bpy

        bpy.ops.wm.open_mainfile(filepath=str(self.source))
        self.output.mkdir(parents=True, exist_ok=True)
        self.prepare_meshes()
        baked_materials = self.bake_materials()
        scene_file = self.output / "scene.gltf"
        bpy.ops.export_scene.gltf(
            filepath=str(scene_file),
            export_format="GLTF_SEPARATE",
            export_texture_dir="textures",
            export_image_format="AUTO",
            export_tangents=True,
            export_cameras=True,
            export_lights=True,
            export_animations=False,
            export_apply=True,
            export_yup=True,
        )
        if not scene_file.is_file():
            raise RuntimeError(f"Blender did not produce {scene_file}")
        print(f"TRUVIS_GLTF_EXPORT_OK {scene_file} baked_materials={len(baked_materials)}", flush=True)

    @staticmethod
    def prepare_meshes():
        import bpy

        bpy.ops.object.select_all(action="DESELECT")
        objects = [obj for obj in bpy.context.scene.objects if obj.type in {"MESH", "CURVE", "SURFACE", "FONT"}]
        for obj in objects:
            obj.hide_set(False)
            obj.select_set(True)
        if objects:
            bpy.context.view_layer.objects.active = objects[0]
            bpy.ops.object.convert(target="MESH")

    def bake_materials(self):
        import bpy

        scene = bpy.context.scene
        texture_dir = self.output / "textures"
        texture_dir.mkdir(parents=True, exist_ok=True)
        scene.render.engine, scene.cycles.samples = "CYCLES", 1
        baked = []

        for material in list(bpy.data.materials):
            if not material.use_nodes:
                continue
            nodes, links = material.node_tree.nodes, material.node_tree.links
            bsdf = next((node for node in nodes if node.type == "BSDF_PRINCIPLED"), None)
            if bsdf is None:
                raise ValueError(f"缺少 Principled BSDF: {material.name}")
            base = bsdf.inputs["Base Color"]
            if not base.is_linked or base.links[0].from_node.type == "TEX_IMAGE":
                continue

            objects = [obj for obj in scene.objects if obj.type == "MESH" and material.name in obj.data.materials]
            if not objects:
                continue
            if any(len(obj.data.materials) != 1 for obj in objects):
                raise ValueError("程序化材质烘焙目前要求每个 mesh 使用单一材质")

            bpy.ops.object.select_all(action="DESELECT")
            for obj in objects:
                obj.select_set(True)
                while obj.data.uv_layers:
                    obj.data.uv_layers.remove(obj.data.uv_layers[0])
                obj.data.uv_layers.new(name="Truvis UV")
            bpy.context.view_layer.objects.active = objects[0]
            bpy.ops.object.mode_set(mode="EDIT")
            bpy.ops.mesh.select_all(action="SELECT")
            bpy.ops.uv.smart_project(angle_limit=math.radians(66), island_margin=0.003)
            bpy.ops.object.mode_set(mode="OBJECT")

            for channel in ("BaseColor", "Normal"):
                if channel == "Normal" and not bsdf.inputs["Normal"].is_linked:
                    continue
                image = bpy.data.images.new(
                    material.name.replace(" ", "_") + "_" + channel,
                    width=self.args.texture_size,
                    height=self.args.texture_size,
                    alpha=False,
                )
                if channel == "Normal":
                    image.colorspace_settings.name = "Non-Color"
                target = nodes.new("ShaderNodeTexImage")
                target.image, nodes.active = image, target
                scene.render.bake.use_clear, scene.render.bake.margin = False, 8
                scene.render.bake.use_pass_direct, scene.render.bake.use_pass_indirect = False, False
                scene.render.bake.use_pass_color = True
                bpy.ops.object.bake(type="DIFFUSE" if channel == "BaseColor" else "NORMAL")
                if channel == "BaseColor":
                    links.new(target.outputs["Color"], base)
                else:
                    normal = nodes.new("ShaderNodeNormalMap")
                    links.new(target.outputs["Color"], normal.inputs["Color"])
                    links.new(normal.outputs["Normal"], bsdf.inputs["Normal"])
                image.filepath_raw = str(texture_dir / (image.name + ".png"))
                image.file_format = "PNG"
                image.save()
                image.pack()
            baked.append(material.name)
        return baked


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--blender",
        type=Path,
        default=Path("C:/Program Files/Blender Foundation/Blender 5.2/blender.exe"),
    )
    parser.add_argument("--texture-size", type=int, default=2048)
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else sys.argv[1:]
    BlenderSceneExporter(parser.parse_args(argv)).run()
