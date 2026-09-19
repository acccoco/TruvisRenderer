"""离线导出 Blender 场景为 glTF、程序化材质贴图和 Truvis manifest。"""
import argparse
import json
import math
import shutil
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
            subprocess.run([
                str(self.args.blender), "--background", "--factory-startup",
                "--python-exit-code", "1", "--python", str(Path(__file__).resolve()),
                "--", "--worker", "--source", str(self.source), "--output", str(self.output),
                "--texture-size", str(self.args.texture_size),
            ], check=True)
            return
        import bpy
        bpy.ops.wm.open_mainfile(filepath=str(self.source))
        self.output.mkdir(parents=True, exist_ok=True)
        scene = bpy.context.scene
        cameras = [self.camera(obj) for obj in scene.objects if obj.type == "CAMERA"]
        if not cameras or scene.camera is None:
            raise ValueError("场景必须有激活的透视相机")
        manifest = {
            "name": self.source.stem, "model": "scene.gltf", "sky": self.world_texture(scene),
            "default_camera": scene.camera.name, "cameras": cameras,
            "area_lights": [self.area_light(obj) for obj in scene.objects if obj.type == "LIGHT"],
        }
        source_objects = len(scene.objects)
        self.prepare_meshes()
        baked = self.bake_materials()
        bpy.ops.export_scene.gltf(
            filepath=str(self.output / "scene.gltf"), export_format="GLTF_SEPARATE",
            export_texture_dir="textures", export_image_format="AUTO", export_tangents=True,
            export_cameras=False, export_lights=False, export_animations=False,
            export_apply=True, export_yup=True,
        )
        gltf = json.loads((self.output / "scene.gltf").read_text(encoding="utf-8"))
        for buffer in gltf.get("buffers", []):
            if (self.output / buffer["uri"]).stat().st_size != buffer["byteLength"]:
                raise ValueError("glTF buffer 长度不匹配")
        for texture in gltf.get("images", []):
            if not (self.output / texture["uri"]).is_file():
                raise ValueError("glTF 外部贴图缺失")
        # 保留可重复导出的源文件；临时转换结果不改写用户的原始 .blend。
        if self.source != self.output / "source.blend":
            shutil.copy2(self.source, self.output / "source.blend")
        audit = {
            "source_objects": source_objects, "mesh_instances": sum("mesh" in n for n in gltf["nodes"]),
            "meshes": len(gltf["meshes"]), "materials": len(gltf["materials"]),
            "images": len(gltf.get("images", [])), "baked_materials": baked,
            "cameras": len(cameras), "area_lights": len(manifest["area_lights"]),
            "area_shape_policy": "disk/ellipse -> equal-area rectangle, preserving flux and direction",
        }
        (self.output / "export.json").write_text(json.dumps(audit, indent=2), encoding="utf-8")
        # manifest 最后发布，成功还必须同时检查进程退出码与 TRUVIS_EXPORT_OK。
        (self.output / "scene.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        print("TRUVIS_EXPORT_OK", json.dumps(audit), flush=True)

    @staticmethod
    def vector(value):
        # Blender Z-up -> glTF / Truvis Y-up，场景单位保持不变。
        return [float(value.x), float(value.z), float(-value.y)]

    def camera(self, obj):
        import bpy
        from mathutils import Matrix
        if obj.data.type != "PERSP" or obj.data.shift_x or obj.data.shift_y:
            raise ValueError(f"不支持非透视或偏移投影相机: {obj.name}")
        q = (Matrix.Rotation(-math.pi / 2, 4, "X") @ obj.matrix_world).to_quaternion().normalized()
        s = bpy.context.scene
        # 从实际投影读取垂直 FOV，避免 sensor_fit 和渲染 aspect 的歧义。
        projection = obj.calc_matrix_camera(
            bpy.context.evaluated_depsgraph_get(), x=s.render.resolution_x, y=s.render.resolution_y,
            scale_x=s.render.pixel_aspect_x, scale_y=s.render.pixel_aspect_y,
        )
        return {
            "name": obj.name, "position": self.vector(obj.matrix_world.translation),
            "rotation": [q.x, q.y, q.z, q.w],
            "fov_deg": math.degrees(2 * math.atan(1 / projection[1][1])), "near": obj.data.clip_start,
        }

    def area_light(self, obj):
        from mathutils import Vector
        light = obj.data
        if light.type != "AREA":
            raise ValueError(f"暂不支持 {light.type} 灯光: {obj.name}")
        if abs(light.spread - math.pi) > 1e-4:
            raise ValueError(f"暂不支持定向 spread 灯光: {obj.name}")
        width = light.size
        height = light.size_y if light.shape in {"RECTANGLE", "ELLIPSE"} else width
        if light.shape in {"DISK", "ELLIPSE"}:
            # 现有 AreaLight 只支持矩形；等面积转换保留通量，软阴影轮廓是近似。
            width *= math.sqrt(math.pi) / 2
            height *= math.sqrt(math.pi) / 2
        basis = obj.matrix_world.to_3x3()
        half_u = basis @ Vector((width / 2, 0, 0))
        half_v = basis @ Vector((0, -height / 2, 0))
        area = 4 * half_u.cross(half_v).length
        if area <= 1e-8:
            raise ValueError(f"灯光面积必须大于零: {obj.name}")
        # Lambert 发射 Phi = pi * area * radiance；不 Normalize 时不除面积。
        radiance = light.energy / (math.pi * area if light.normalize else math.pi)
        return {
            "center": self.vector(obj.matrix_world.translation),
            "half_u": self.vector(half_u), "half_v": self.vector(half_v),
            "radiance": [float(c * radiance) for c in light.color],
        }

    def world_texture(self, scene):
        import bpy
        if scene.world is None:
            return None
        background = next((n for n in scene.world.node_tree.nodes if n.type == "BACKGROUND"), None)
        if background is None or any(s.is_linked for s in background.inputs):
            raise ValueError("导出器目前只接受恒定 World Background")
        strength, color = background.inputs["Strength"].default_value, background.inputs["Color"].default_value
        image = bpy.data.images.new("World radiance", width=4, height=2, float_buffer=True)
        image.pixels[:] = [color[0]*strength, color[1]*strength, color[2]*strength, 1.0] * 8
        image.filepath_raw, image.file_format = str(self.output / "world.hdr"), "HDR"
        image.save()
        return "world.hdr"

    @staticmethod
    def prepare_meshes():
        import bpy
        bpy.ops.object.select_all(action="DESELECT")
        objects = [o for o in bpy.context.scene.objects if o.type in {"MESH", "CURVE", "SURFACE", "FONT"}]
        for obj in objects:
            obj.hide_set(False)
            obj.select_set(True)
        bpy.context.view_layer.objects.active = objects[0]
        # 曲线/修改器先求值，材质烘焙与最终 glTF 使用相同几何及 UV。
        bpy.ops.object.convert(target="MESH")

    def bake_materials(self):
        import bpy
        scene = bpy.context.scene
        texture_dir = self.output / "textures"
        texture_dir.mkdir(parents=True, exist_ok=True)
        scene.render.engine, scene.cycles.samples = "CYCLES", 1
        prefs = bpy.context.preferences.addons["cycles"].preferences
        try:
            prefs.compute_device_type = "OPTIX"
            prefs.get_devices()
            for device in prefs.devices:
                device.use = device.type == "OPTIX"
            scene.cycles.device = "GPU"
        except Exception:
            scene.cycles.device = "CPU"
        baked = []
        for material in list(bpy.data.materials):
            if not material.use_nodes:
                continue
            nodes, links = material.node_tree.nodes, material.node_tree.links
            bsdf = next((n for n in nodes if n.type == "BSDF_PRINCIPLED"), None)
            if bsdf is None:
                raise ValueError(f"缺少 Principled BSDF: {material.name}")
            base = bsdf.inputs["Base Color"]
            if not base.is_linked or base.links[0].from_node.type == "TEX_IMAGE":
                continue
            objects = [o for o in scene.objects if o.type == "MESH" and material.name in o.data.materials]
            if not objects:
                continue
            if any(len(o.data.materials) != 1 for o in objects):
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
            print("BAKE", material.name, len(objects), flush=True)
            for channel in ("BaseColor", "Normal"):
                if channel == "Normal" and not bsdf.inputs["Normal"].is_linked:
                    continue
                image = bpy.data.images.new(
                    material.name.replace(" ", "_") + "_" + channel,
                    width=self.args.texture_size, height=self.args.texture_size, alpha=False,
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
                image.filepath_raw, image.file_format = str(texture_dir / (image.name + ".png")), "PNG"
                image.save()
                image.pack()
            baked.append(material.name)
        return baked


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--blender", type=Path, default=Path("C:/Program Files/Blender Foundation/Blender 5.2/blender.exe"))
    parser.add_argument("--texture-size", type=int, default=2048)
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    BlenderSceneExporter(parser.parse_args(argv)).run()
