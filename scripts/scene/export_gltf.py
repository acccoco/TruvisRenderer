"""将 Blender 场景导出为使用外部贴图的独立 glTF 场景。

宿主 Python 进程负责转换设置和 Blender 子进程；worker 分支由 Blender
执行，只写入请求的输出目录。源场景和源贴图保持只读。
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path


class SceneExporter:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.source = args.source.resolve()
        self.output_dir = args.output_dir.resolve()
        self.staged_texture_dir = self.output_dir / "_source_textures"

    def run(self) -> None:
        if self.args.worker:
            self.run_worker()
            return

        self.output_dir.mkdir(parents=True, exist_ok=True)
        scene_file = self.output_dir / "scene.gltf"
        if scene_file.exists():
            scene_file.unlink()
        self.stage_textures()

        command = [
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
            "--output-dir",
            str(self.output_dir),
            "--texture-size",
            str(self.args.texture_size),
            "--staged-texture-dir",
            str(self.staged_texture_dir),
        ]
        subprocess.run(command, check=True)
        if not scene_file.is_file():
            raise RuntimeError(f"Blender did not produce {scene_file}")

        if self.staged_texture_dir.exists():
            shutil.rmtree(self.staged_texture_dir)

    def stage_textures(self) -> None:
        texture_root = self.args.texture_root
        if texture_root is None:
            candidate = self.source.parent / "Textures"
            texture_root = candidate if candidate.is_dir() else None
        if texture_root is None:
            return
        texture_root = texture_root.resolve()

        source_files = sorted(
            {
                source
                for pattern in ("*.dds", "*.png", "*.jpg", "*.jpeg", "*.tga")
                for source in texture_root.glob(pattern)
            }
        )
        if not source_files:
            return

        try:
            from PIL import Image, ImageOps
        except ImportError as exc:
            raise RuntimeError("image staging requires Pillow") from exc

        self.staged_texture_dir.mkdir(parents=True, exist_ok=True)
        for source in source_files:
            target = self.staged_texture_dir / f"{source.stem}.png"
            with Image.open(source) as original:
                image = original.convert("RGBA")
                if self.args.invert_normal_green and "normal" in source.stem.lower():
                    red, green, blue, alpha = image.split()
                    image = Image.merge("RGBA", (red, ImageOps.invert(green), blue, alpha))
                image.save(target, compress_level=2)

    def run_worker(self) -> None:
        import bpy

        bpy.ops.wm.read_factory_settings(use_empty=True)
        if self.source.suffix.lower() == ".blend":
            bpy.ops.wm.open_mainfile(filepath=str(self.source))
        elif self.source.suffix.lower() == ".fbx":
            bpy.ops.import_scene.fbx(filepath=str(self.source), use_anim=False)
        else:
            raise ValueError(f"unsupported scene source: {self.source.suffix}")

        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.replace_staged_images()
        self.prepare_meshes()
        baked_materials = self.bake_procedural_materials()
        scene_file = self.output_dir / "scene.gltf"
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
        print(
            f"TRUVIS_GLTF_EXPORT_OK {scene_file} baked_materials={len(baked_materials)}",
            flush=True,
        )

    def replace_staged_images(self) -> None:
        if not self.args.staged_texture_dir.is_dir():
            return
        import bpy

        staged = {
            path.stem.lower(): path
            for path in self.args.staged_texture_dir.glob("*.png")
        }
        for image in bpy.data.images:
            if image.source in {"GENERATED", "VIEWER"}:
                continue
            source = Path(image.filepath)
            replacement = staged.get(source.stem.lower())
            if replacement is not None:
                image.filepath = str(replacement)
                image.reload()

    @staticmethod
    def prepare_meshes() -> None:
        import bpy

        bpy.ops.object.select_all(action="DESELECT")
        objects = [
            obj
            for obj in bpy.context.scene.objects
            if obj.type in {"MESH", "CURVE", "SURFACE", "FONT"}
        ]
        for obj in objects:
            obj.hide_set(False)
            obj.select_set(True)
        if objects:
            bpy.context.view_layer.objects.active = objects[0]
            bpy.ops.object.convert(target="MESH")

    def bake_procedural_materials(self) -> list[str]:
        import bpy

        scene = bpy.context.scene
        texture_dir = self.output_dir / "textures"
        texture_dir.mkdir(parents=True, exist_ok=True)
        scene.render.engine, scene.cycles.samples = "CYCLES", 1
        baked: list[str] = []

        for material in list(bpy.data.materials):
            if not material.use_nodes:
                continue
            nodes, links = material.node_tree.nodes, material.node_tree.links
            bsdf = next((node for node in nodes if node.type == "BSDF_PRINCIPLED"), None)
            if bsdf is None:
                raise ValueError(f"missing Principled BSDF: {material.name}")

            base = bsdf.inputs["Base Color"]
            if not base.is_linked or base.links[0].from_node.type == "TEX_IMAGE":
                continue

            objects = [
                obj
                for obj in scene.objects
                if obj.type == "MESH" and material.name in obj.data.materials
            ]
            if not objects:
                continue
            if any(len(obj.data.materials) != 1 for obj in objects):
                raise ValueError(
                    "procedural material baking requires one material per mesh"
                )

            bpy.ops.object.select_all(action="DESELECT")
            for obj in objects:
                obj.select_set(True)
                while obj.data.uv_layers:
                    obj.data.uv_layers.remove(obj.data.uv_layers[0])
                obj.data.uv_layers.new(name="Truvis UV")
            bpy.context.view_layer.objects.active = objects[0]
            bpy.ops.object.mode_set(mode="EDIT")
            bpy.ops.mesh.select_all(action="SELECT")
            bpy.ops.uv.smart_project(angle_limit=1.151917, island_margin=0.003)
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
                scene.render.bake.use_clear = False
                scene.render.bake.margin = 8
                scene.render.bake.use_pass_direct = False
                scene.render.bake.use_pass_indirect = False
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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument(
        "--blender",
        type=Path,
        default=Path("C:/Program Files/Blender Foundation/Blender 5.2/blender.exe"),
    )
    parser.add_argument("--texture-size", type=int, default=2048)
    parser.add_argument("--texture-root", type=Path)
    parser.add_argument("--invert-normal-green", action="store_true")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--staged-texture-dir", type=Path, help=argparse.SUPPRESS)
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else sys.argv[1:]
    args = parser.parse_args(argv)
    if args.worker and args.staged_texture_dir is None:
        raise ValueError("worker mode requires --staged-texture-dir")
    if not args.source.is_file():
        raise FileNotFoundError(args.source)
    return args


if __name__ == "__main__":
    SceneExporter(parse_args()).run()
