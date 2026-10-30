"""将 Blender 场景导出为使用外部贴图的独立 glTF 场景。

宿主 Python 进程负责转换设置和 Blender 子进程；worker 分支由 Blender
执行，只写入请求的输出目录。源场景和源贴图保持只读。
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


class SceneExporter:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.source = args.source.resolve()
        self.output_dir = args.output_dir.resolve()
        self.publish_dir = (args.publish_dir if args.worker and args.publish_dir else self.output_dir).resolve()
        self.staged_texture_dir: Path | None = args.staged_texture_dir

    def run(self) -> None:
        self.validate_inputs()
        if self.args.worker:
            self.run_worker()
            return

        from validate_gltf import GltfValidator

        self.output_dir.mkdir(parents=True, exist_ok=True)
        # 临时目录只由本次调用创建和回收；不能删除用户已有的固定名称目录。
        with tempfile.TemporaryDirectory(prefix=".truvis-export-", dir=self.output_dir) as temporary:
            working_dir = Path(temporary)
            self.staged_texture_dir = working_dir / "source-textures"
            self.stage_textures()
            worker_output = working_dir / "result"
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
                str(worker_output),
                "--publish-dir",
                str(self.output_dir),
                "--texture-size",
                str(self.args.texture_size),
                "--staged-texture-dir",
                str(self.staged_texture_dir),
            ]
            subprocess.run(command, check=True)
            GltfValidator(worker_output / "scene.gltf").validate()
            self.publish_scene(worker_output)
        print(f"TRUVIS_GLTF_EXPORT_OK {self.output_dir / 'scene.gltf'}", flush=True)

    def validate_inputs(self) -> None:
        if not self.source.is_file():
            raise FileNotFoundError(self.source)
        if self.source.suffix.lower() not in {".blend", ".fbx"}:
            raise ValueError(f"unsupported scene source: {self.source.suffix}")
        if self.args.texture_size <= 0:
            raise ValueError("texture size must be positive")
        if self.source.is_relative_to(self.publish_dir):
            raise ValueError("output directory must not contain the source scene")

    def publish_scene(self, worker_output: Path) -> None:
        files = sorted(path for path in worker_output.rglob("*") if path.is_file())
        # 先核对全部目标，再写资源，最后发布 manifest；不删除其它已有文件。
        files.sort(key=lambda path: path.name == "scene.gltf")
        for source in files:
            target = self.output_dir / source.relative_to(worker_output)
            if not target.resolve().is_relative_to(self.output_dir) or target.is_symlink():
                raise ValueError(f"export target redirects outside its owned path: {target}")
            if target.exists() and not target.is_file():
                raise ValueError(f"export target is not a file: {target}")
            for parent in target.parents:
                if parent == self.output_dir:
                    break
                if parent.exists() and not parent.is_dir():
                    raise ValueError(f"export target parent is not a directory: {parent}")
        for source in files:
            target = self.output_dir / source.relative_to(worker_output)
            target.parent.mkdir(parents=True, exist_ok=True)
            source.replace(target)

    def stage_textures(self) -> None:
        texture_root = self.args.texture_root
        if texture_root is None:
            candidate = self.source.parent / "Textures"
            texture_root = candidate if candidate.is_dir() else None
        if texture_root is None:
            return
        texture_root = texture_root.resolve()
        if not texture_root.is_dir():
            raise ValueError(f"texture root is not a directory: {texture_root}")
        if texture_root.is_relative_to(self.publish_dir) or self.publish_dir.is_relative_to(texture_root):
            raise ValueError("output directory and source texture root must not overlap")

        source_files = sorted(
            source for source in texture_root.iterdir()
            if source.is_file() and source.suffix.lower() in {".dds", ".png", ".jpg", ".jpeg", ".tga"}
        )
        if not source_files:
            return
        names: set[str] = set()
        for source in source_files:
            name = source.stem.casefold()
            if name in names:
                raise ValueError(f"ambiguous staged texture name: {source.stem}")
            names.add(name)

        try:
            from PIL import Image, ImageOps
        except ImportError as exc:
            raise RuntimeError("image staging requires Pillow") from exc

        assert self.staged_texture_dir is not None
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

        for image in bpy.data.images:
            if image.source not in {"GENERATED", "VIEWER"} and image.filepath:
                path = Path(bpy.path.abspath(image.filepath, library=image.library)).resolve()
                if path.is_relative_to(self.publish_dir):
                    raise ValueError(f"output directory contains source image: {path}")
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
            # prepare_meshes 会改变 Blender 的选择状态；导出整棵 scene，避免漏掉 camera、light 或未选中的 mesh。
            use_selection=False,
            # Blender 生成的 tangent 基于 active UV；烘焙 normal 使用独立 UV，交给 glTF 消费端重建。
            export_tangents=not baked_materials,
            export_cameras=True,
            export_lights=True,
            export_animations=False,
            export_skins=False,
            export_vertex_color="NONE",
            export_all_vertex_colors=False,
            # mesh 转换和材质烘焙已使用当前帧；节点变换不能在导出时跳回第 0 帧。
            export_current_frame=True,
            export_apply=True,
            export_yup=True,
        )
        if not scene_file.is_file():
            raise RuntimeError(f"Blender did not produce {scene_file}")
        print(
            f"TRUVIS_GLTF_WORKER_OK {scene_file} baked_materials={len(baked_materials)}",
            flush=True,
        )

    def replace_staged_images(self) -> None:
        if self.staged_texture_dir is None or not self.staged_texture_dir.is_dir():
            return
        import bpy

        staged = {
            path.stem.casefold(): path
            for path in self.staged_texture_dir.glob("*.png")
        }
        for image in bpy.data.images:
            if image.source in {"GENERATED", "VIEWER"}:
                continue
            source = Path(image.filepath)
            replacement = staged.get(source.stem.casefold())
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

        for obj in list(scene.objects):
            if obj.type != "MESH":
                continue
            for slot in obj.material_slots:
                material = slot.material
                if material is None or not material.use_nodes:
                    continue
                bsdf = self.material_bsdf(material)
                base = bsdf.inputs["Base Color"]
                if not base.is_linked or base.links[0].from_node.type == "TEX_IMAGE":
                    continue
                if len(obj.material_slots) != 1:
                    raise ValueError("procedural material baking requires one material per mesh")

                # Object Info 等输入依赖对象；复制仅存在于 worker，避免共享图互相覆盖或再次烘焙。
                if obj.data.users > 1:
                    obj.data = obj.data.copy()
                material = material.copy()
                obj.material_slots[0].material = material
                self.bake_material(obj, material, len(baked), texture_dir)
                baked.append(material.name)
        return baked

    @staticmethod
    def material_bsdf(material):
        output = material.node_tree.get_output_node("CYCLES")
        if output != material.node_tree.get_output_node("EEVEE"):
            raise ValueError(f"material must use the same output for Cycles and Eevee: {material.name}")
        if output is not None and output.inputs["Surface"].is_linked:
            bsdf = output.inputs["Surface"].links[0].from_node
            if bsdf.type == "BSDF_PRINCIPLED":
                return bsdf
        raise ValueError(f"material output must connect directly to Principled BSDF: {material.name}")

    def bake_material(self, obj, material, material_index: int, texture_dir: Path) -> None:
        import bpy

        nodes, links = material.node_tree.nodes, material.node_tree.links
        bsdf = self.material_bsdf(material)
        base = bsdf.inputs["Base Color"]
        output = material.node_tree.get_output_node("CYCLES")
        source_uv = next((uv for uv in obj.data.uv_layers if uv.active_render), None)
        target_uv = obj.data.uv_layers.new(name="Truvis UV")
        if target_uv is None:
            raise ValueError(f"cannot add baking UV layer: {obj.name}")
        obj.data.uv_layers.active = target_uv
        if source_uv is not None:
            source_uv.active_render = True
        bpy.ops.object.select_all(action="DESELECT")
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.uv.smart_project(angle_limit=1.151917, island_margin=0.003)
        bpy.ops.object.mode_set(mode="OBJECT")

        # UV layer 的 RNA 引用可能在 edit mode 重建后失效，只保留稳定名称。
        bake_uv = obj.data.uv_layers.active.name
        uv_node = nodes.new("ShaderNodeUVMap")
        uv_node.uv_map = bake_uv
        targets = {}
        for channel in ("BaseColor", "Normal"):
            if channel == "Normal" and not bsdf.inputs["Normal"].is_linked:
                continue
            image = bpy.data.images.new(
                f"material_{material_index}_{channel}",
                width=self.args.texture_size,
                height=self.args.texture_size,
                alpha=False,
            )
            if channel == "Normal":
                image.colorspace_settings.name = "Non-Color"
            target = nodes.new("ShaderNodeTexImage")
            target.image, nodes.active = image, target
            links.new(uv_node.outputs["UV"], target.inputs["Vector"])
            emission = None
            try:
                if channel == "BaseColor":
                    # 只求 Base Color 表达式；DIFFUSE pass 会受 metallic 等 BSDF 参数影响。
                    emission = nodes.new("ShaderNodeEmission")
                    links.new(base.links[0].from_socket, emission.inputs["Color"])
                    links.new(emission.outputs[0], output.inputs["Surface"])
                bpy.ops.object.bake(
                    type="EMIT" if channel == "BaseColor" else "NORMAL",
                    uv_layer=bake_uv,
                    use_selected_to_active=False,
                    use_cage=False,
                    use_clear=True,
                    margin=8,
                    margin_type="EXTEND",
                    target="IMAGE_TEXTURES",
                    normal_space="TANGENT",
                    normal_r="POS_X", normal_g="POS_Y", normal_b="POS_Z",
                )
            finally:
                if emission is not None:
                    links.new(bsdf.outputs["BSDF"], output.inputs["Surface"])
                    nodes.remove(emission)
            image.filepath_raw = str(texture_dir / (image.name + ".png"))
            image.file_format = "PNG"
            image.save()
            image.pack()
            targets[channel] = target

        # 两次 bake 都读取原图；完成后才替换输入，避免 Normal 间接依赖 Base Color 时读到新图。
        links.new(targets["BaseColor"].outputs["Color"], base)
        if "Normal" in targets:
            normal = nodes.new("ShaderNodeNormalMap")
            normal.uv_map = bake_uv
            links.new(targets["Normal"].outputs["Color"], normal.inputs["Color"])
            links.new(normal.outputs["Normal"], bsdf.inputs["Normal"])


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
    parser.add_argument("--publish-dir", type=Path, help=argparse.SUPPRESS)
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else sys.argv[1:]
    args = parser.parse_args(argv)
    if args.worker and args.staged_texture_dir is None:
        raise ValueError("worker mode requires --staged-texture-dir")
    if not args.source.is_file():
        raise FileNotFoundError(args.source)
    return args


if __name__ == "__main__":
    SceneExporter(parse_args()).run()
