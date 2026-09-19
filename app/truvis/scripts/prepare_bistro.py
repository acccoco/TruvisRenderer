"""将完整 ORCA Bistro 包准备成使用外部 PNG 贴图的 glTF。

以 Python + Pillow 运行；Blender 仅用于离线导出 FBX 几何与材质。
源包保持只读，运行时加载不依赖 Blender。
"""
import argparse
import json
import math
import shutil
import subprocess
import sys
from pathlib import Path


class BistroPreparer:
    SCENES = ('BistroExterior', 'BistroInterior', 'BistroInterior_Wine')
    LIQUID_IOR = {'Water': 1.33, 'Ice': 1.31, 'White_Wine': 1.33, 'Red_Wine': 1.33, 'Beer': 1.33}

    def __init__(self, args):
        self.args = args
        self.source = args.source.resolve()
        self.output = args.output.resolve()

    def prepare(self):
        from PIL import Image, ImageOps
        self.output.mkdir(parents=True, exist_ok=True)
        textures = self.output / '_source_textures'
        textures.mkdir(exist_ok=True)
        alpha = {}
        for file in sorted((self.source / 'Textures').glob('*.dds')):
            target = textures / (file.stem + '.png')
            with Image.open(file) as original:
                image = original.convert('RGBA')
                if file.stem.lower().endswith('_normal'):
                    r, g, b, a = image.split()
                    image = Image.merge('RGBA', (r, ImageOps.invert(g), b, a))
                alpha[file.stem] = image.getchannel('A').getextrema()[0] / 255.0
                if not target.exists() or target.stat().st_mtime < file.stat().st_mtime:
                    image.save(target, compress_level=2)
        (self.output / 'texture_alpha.json').write_text(json.dumps(alpha), encoding='utf-8')
        for name in ('LICENSE.txt', 'README.txt', 'san_giuseppe_bridge_4k.hdr'):
            shutil.copy2(self.source / name, self.output / name)
        for scene in self.SCENES:
            subprocess.run([str(self.args.blender), '--background', '--factory-startup', '--python', str(Path(__file__).resolve()),
                            '--', '--source', str(self.source), '--output', str(self.output), '--worker', scene], check=True)
        print(f'Prepared all three Bistro scenes in {self.output}', flush=True)

    def export_scene(self):
        import bpy
        from mathutils import Vector
        bpy.ops.wm.read_factory_settings(use_empty=True)
        bpy.ops.import_scene.fbx(filepath=str(self.source / (self.args.worker + '.fbx')), use_anim=False)
        alpha = json.loads((self.output / 'texture_alpha.json').read_text(encoding='utf-8'))
        properties = self.material_properties()
        for material in bpy.data.materials:
            self.prepare_material(material, alpha, properties.get(material.name.split('.')[0], {}))
        cameras = [obj for obj in bpy.context.scene.objects if obj.type == 'CAMERA']
        if not cameras:
            raise RuntimeError('Bistro FBX did not contain a camera')
        camera = cameras[0]
        # Blender Z-up 转为 glTF Y-up；Runtime 不改变场景单位与坐标约定。
        p = camera.matrix_world.translation
        direction = camera.matrix_world.to_quaternion() @ Vector((0, 0, -1))
        forward = Vector((direction.x, direction.z, -direction.y)).normalized()
        view = {'position': [p.x, p.z, -p.y],
                'yaw_deg': math.degrees(math.atan2(-forward.x, -forward.z)),
                'pitch_deg': math.degrees(math.asin(forward.y)),
                'fov_deg': math.degrees(camera.data.angle_y)}
        (self.output / (self.args.worker + '.camera.json')).write_text(json.dumps(view, indent=2), encoding='utf-8')
        bpy.ops.export_scene.gltf(filepath=str(self.output / (self.args.worker + '.gltf')),
                                  export_format='GLTF_SEPARATE', export_texture_dir='textures',
                                  export_image_format='AUTO', export_cameras=True, export_lights=False,
                                  export_animations=False, export_apply=True, export_yup=True)
        # Blender 4.4 把 dithered coverage 导出为 BLEND；源树叶/围栏属于 cutout，
        # 明确写成 MASK，供 raster/RT 共用的 alpha-test 路径读取。
        scene_file = self.output / (self.args.worker + '.gltf')
        scene = json.loads(scene_file.read_text(encoding='utf-8'))
        for material in scene.get('materials', []):
            if material.get('alphaMode') == 'BLEND':
                material['alphaMode'] = 'MASK'
                material['alphaCutoff'] = 0.5
            # 源玻璃的 diffuse 不表示透射吸收；当前模型会用 base color 调制每次折射，
            # 因此映射为清透基色，避免把深色 diffuse 反复相乘后得到黑色杯体/窗户。
            if 'glass' in material.get('name', '').lower():
                pbr = material['pbrMetallicRoughness']
                pbr.pop('baseColorTexture', None)
                pbr['baseColorFactor'] = [1.0, 1.0, 1.0, 1.0]
        scene_file.write_text(json.dumps(scene, indent=2), encoding='utf-8')
        print('BISTRO_EXPORT_COMPLETE', self.args.worker, json.dumps(view), flush=True)

    def material_properties(self):
        # 复用 Blender 的 FBX parser，补回其 Principled 转换未保留的常量 emission。
        from io_scene_fbx import parse_fbx
        root, _ = parse_fbx.parse(str(self.source / (self.args.worker + '.fbx')))
        objects = next(element for element in root.elems if element.id == b'Objects')
        return {material.props[1].split(b'\x00')[0].decode():
                {prop.props[0].decode(): prop.props[4:] for child in material.elems
                 if child.id == b'Properties70' for prop in child.elems if prop.id == b'P'}
                for material in objects.elems if material.id == b'Material'}

    def prepare_material(self, material, alpha, properties):
        import bpy
        images = {Path(node.image.filepath).stem.rsplit('_', 1)[-1].lower(): Path(node.image.filepath).stem
                  for node in material.node_tree.nodes if node.type == 'TEX_IMAGE' and node.image} if material.node_tree else {}
        old = next((node for node in material.node_tree.nodes if node.type == 'BSDF_PRINCIPLED'), None) if material.node_tree else None
        opacity = old.inputs['Alpha'].default_value if old else 1.0
        emission_color = properties.get('Emissive', [0.0, 0.0, 0.0])
        material.use_nodes = True
        nodes, links = material.node_tree.nodes, material.node_tree.links
        nodes.clear()
        bsdf = nodes.new('ShaderNodeBsdfPrincipled')
        output = nodes.new('ShaderNodeOutputMaterial')
        links.new(bsdf.outputs['BSDF'], output.inputs['Surface'])
        bsdf.inputs['Metallic'].default_value = 0.0
        bsdf.inputs['Roughness'].default_value = 0.5
        bsdf.inputs['Specular IOR Level'].default_value = 0.5
        base = self.image_node(nodes, images.get('basecolor'), False)
        if base:
            links.new(base.outputs['Color'], bsdf.inputs['Base Color'])
        else:
            # 玻璃/液体的 DiffuseFactor 为 0；保留颜色用于现有 transmission tint，体吸收另有契约。
            color = properties.get('DiffuseColor', [1.0, 1.0, 1.0])
            bsdf.inputs['Base Color'].default_value = (*color, 1.0)
        orm = self.image_node(nodes, images.get('specular'), True)
        if orm:
            channels = nodes.new('ShaderNodeSeparateColor')
            links.new(orm.outputs['Color'], channels.inputs['Color'])
            links.new(channels.outputs['Green'], bsdf.inputs['Roughness'])
            links.new(channels.outputs['Blue'], bsdf.inputs['Metallic'])
        normal = self.image_node(nodes, images.get('normal'), True)
        if normal:
            mapping = nodes.new('ShaderNodeNormalMap')
            links.new(normal.outputs['Color'], mapping.inputs['Color'])
            links.new(mapping.outputs['Normal'], bsdf.inputs['Normal'])
        emissive = self.image_node(nodes, images.get('emissive'), False)
        if emissive:
            links.new(emissive.outputs['Color'], bsdf.inputs['Emission Color'])
            # ORCA Wine preset 将 FBX 的 0.5 emission factor 乘以 1000；
            # 两个室内版本采用相同光源强度量级。
            bsdf.inputs['Emission Strength'].default_value = max(emission_color) * (1 if self.args.worker == 'BistroExterior' else 1000)
        else:
            bsdf.inputs['Emission Color'].default_value = (*emission_color, 1.0)
            bsdf.inputs['Emission Strength'].default_value = 1.0 if self.args.worker == 'BistroExterior' else 1000.0
        name = material.name.split('.')[0]
        glass = 'glass' in name.lower() or name in self.LIQUID_IOR or opacity < 0.99
        if glass:
            bsdf.inputs['Transmission Weight'].default_value = 1.0
            bsdf.inputs['IOR'].default_value = self.LIQUID_IOR.get(name, 1.55)
            for socket in ('Metallic', 'Roughness'):
                for link in list(bsdf.inputs[socket].links):
                    links.remove(link)
            bsdf.inputs['Metallic'].default_value = 0.0
            bsdf.inputs['Roughness'].default_value = 0.1 if name == 'Ice' else 0.0
        elif base and alpha.get(images['basecolor'], 1) < 0.5:
            links.new(base.outputs['Alpha'], bsdf.inputs['Alpha'])
            material.surface_render_method = 'DITHERED'
        material.use_backface_culling = False

    def image_node(self, nodes, stem, linear):
        import bpy
        if not stem:
            return None
        image = bpy.data.images.load(str(self.output / '_source_textures' / (stem + '.png')), check_existing=True)
        image.colorspace_settings.name = 'Non-Color' if linear else 'sRGB'
        node = nodes.new('ShaderNodeTexImage')
        node.image = image
        return node


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parents[3] / 'assets/gltf/bistro')
    parser.add_argument('--blender', type=Path, default=Path(r'C:\Program Files\Blender Foundation\Blender 4.4\blender.exe'))
    parser.add_argument('--worker', choices=BistroPreparer.SCENES)
    args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else None)
    preparer = BistroPreparer(args)
    if args.worker:
        preparer.export_scene()
    else:
        preparer.prepare()
