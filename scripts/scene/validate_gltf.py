"""预检独立 glTF 2.0 的外部资源、buffer 范围与核心引用；不替代完整格式或运行时验收。"""

from __future__ import annotations

import argparse
import json
import math
import struct
import re
import sys
from pathlib import Path, PureWindowsPath
from urllib.parse import unquote_to_bytes, urlsplit


class GltfValidationError(RuntimeError):
    pass


class GltfValidator:
    def __init__(self, scene: Path) -> None:
        self.scene = scene.resolve()
        self.root = self.scene.parent
        self.document: dict = {}
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.buffers: dict[int, bytes] = {}

    def validate(self) -> None:
        self.document = {}
        self.errors.clear()
        self.warnings.clear()
        self.buffers.clear()
        try:
            self.document = json.loads(
                self.scene.read_text(encoding="utf-8"), parse_constant=self.reject_json_constant
            )
        except (OSError, UnicodeError, json.JSONDecodeError, RecursionError) as exc:
            raise GltfValidationError(f"cannot read glTF JSON: {exc}") from exc
        self.require_object(self.document, "glTF root")
        asset = self.require_object(self.document.get("asset"), "asset")
        self.validate_version(asset)
        self.validate_finite_json(self.document)
        self.validate_extensions()
        for name in (
            "buffers", "bufferViews", "accessors", "images", "textures", "samplers",
            "materials", "meshes", "nodes", "scenes", "skins", "cameras", "animations",
        ):
            for index, item in enumerate(self.require_array(self.document.get(name, []), name)):
                self.require_object(item, f"{name}[{index}]")

        self.validate_external_resources()
        self.validate_references()
        if not self.errors:
            self.validate_accessors()
            self.validate_semantics()
        if self.errors:
            raise GltfValidationError("\n".join(self.errors))

    @staticmethod
    def reject_json_constant(value: str) -> None:
        raise GltfValidationError(f"invalid JSON numeric constant: {value}")

    @staticmethod
    def require_object(value: object, label: str) -> dict:
        if not isinstance(value, dict):
            raise GltfValidationError(f"{label} must be an object")
        return value

    @staticmethod
    def require_array(value: object, label: str) -> list:
        if not isinstance(value, list):
            raise GltfValidationError(f"{label} must be an array")
        return value

    def validate_external_resources(self) -> None:
        for kind in ("buffers", "images"):
            for index, item in enumerate(self.document.get(kind, [])):
                uri = item.get("uri")
                if not isinstance(uri, str):
                    self.errors.append(f"{kind}[{index}] must use an external URI")
                    continue
                resolved = self.resolve_uri(uri)
                if resolved is None:
                    continue
                try:
                    if not resolved.is_file():
                        self.errors.append(f"missing {kind}[{index}] URI: {uri}")
                    elif kind == "buffers":
                        length = item.get("byteLength")
                        if self.check_uint(length, f"buffers[{index}].byteLength", minimum=1):
                            if resolved.stat().st_size < length:
                                self.errors.append(f"buffers[{index}] file is shorter than byteLength")
                            else:
                                self.buffers[index] = resolved.read_bytes()[:length]
                except (OSError, ValueError) as exc:
                    self.errors.append(f"cannot read {kind}[{index}] URI {uri!r}: {exc}")

    def resolve_uri(self, uri: str) -> Path | None:
        try:
            if not uri or re.search(r"%(?![0-9a-fA-F]{2})|[\x00-\x1f\x7f]", uri):
                raise ValueError("empty URI, invalid percent escape or control character")
            parsed = urlsplit(uri)
            if parsed.scheme or parsed.netloc or "?" in uri or "#" in uri:
                raise ValueError("expected a relative file URI without query or fragment")
            # 仅解码一次；不能把损坏 UTF-8 替换成另一文件名，或再次解释编码后的 %。
            decoded = unquote_to_bytes(uri).decode("utf-8")
            if re.search(r"[\x00-\x1f\x7f\\:]", decoded) or PureWindowsPath(decoded).anchor:
                raise ValueError("expected a relative file path without controls or drive syntax")
            candidate = (self.root / decoded).resolve()
            if not candidate.is_relative_to(self.root):
                raise ValueError("URI escapes scene directory")
        except (OSError, ValueError, UnicodeError, RuntimeError) as exc:
            self.errors.append(f"invalid URI {uri!r}: {exc}")
            return None
        return candidate

    def validate_references(self) -> None:
        accessors = self.document.get("accessors", [])
        buffer_views = self.document.get("bufferViews", [])
        buffers = self.document.get("buffers", [])
        meshes = self.document.get("meshes", [])
        materials = self.document.get("materials", [])
        textures = self.document.get("textures", [])
        images = self.document.get("images", [])

        for index, accessor in enumerate(accessors):
            if "bufferView" in accessor:
                self.check_index(accessor["bufferView"], buffer_views, f"accessors[{index}].bufferView")
            if "sparse" in accessor:
                label = f"accessors[{index}].sparse"
                sparse = self.require_object(accessor["sparse"], label)
                for field in ("indices", "values"):
                    info = self.require_object(sparse.get(field), f"{label}.{field}")
                    self.check_index(info.get("bufferView"), buffer_views, f"{label}.{field}.bufferView")
        for index, view in enumerate(buffer_views):
            label = f"bufferViews[{index}]"
            valid_buffer = self.check_index(view.get("buffer"), buffers, f"{label}.buffer")
            valid_offset = self.check_uint(view.get("byteOffset", 0), f"{label}.byteOffset")
            valid_length = self.check_uint(view.get("byteLength"), f"{label}.byteLength", minimum=1)
            if valid_buffer and valid_offset and valid_length:
                length = buffers[view["buffer"]].get("byteLength")
                if type(length) is int and view.get("byteOffset", 0) + view["byteLength"] > length:
                    self.errors.append(f"{label} exceeds its buffer byteLength")
        for index, texture in enumerate(textures):
            if "source" in texture:
                self.check_index(texture["source"], images, f"textures[{index}].source")
            if "sampler" in texture:
                self.check_index(
                    texture["sampler"],
                    self.document.get("samplers", []),
                    f"textures[{index}].sampler",
                )
        for index, material in enumerate(materials):
            label = f"materials[{index}]"
            pbr = self.require_object(
                material.get("pbrMetallicRoughness", {}), f"{label}.pbrMetallicRoughness"
            )
            for owner, name in (
                (pbr, "baseColorTexture"), (pbr, "metallicRoughnessTexture"),
                (material, "normalTexture"), (material, "occlusionTexture"), (material, "emissiveTexture"),
            ):
                if name in owner:
                    info = self.require_object(owner[name], f"{label}.{name}")
                    self.check_index(info.get("index"), textures, f"{label}.{name}.index")
        for index, mesh in enumerate(meshes):
            primitives = self.require_array(mesh.get("primitives"), f"meshes[{index}].primitives")
            for primitive_index, primitive in enumerate(primitives):
                label = f"meshes[{index}].primitives[{primitive_index}]"
                self.require_object(primitive, label)
                attributes = self.require_object(primitive.get("attributes"), f"{label}.attributes")
                for semantic, accessor_index in attributes.items():
                    self.check_index(
                        accessor_index,
                        accessors,
                        f"meshes[{index}].primitives[{primitive_index}].attributes.{semantic}",
                    )
                if "indices" in primitive:
                    self.check_index(
                        primitive["indices"],
                        accessors,
                        f"meshes[{index}].primitives[{primitive_index}].indices",
                    )
                if "material" in primitive:
                    self.check_index(
                        primitive["material"],
                        materials,
                        f"meshes[{index}].primitives[{primitive_index}].material",
                    )
                targets = self.require_array(primitive.get("targets", []), f"{label}.targets")
                for target_index, target in enumerate(targets):
                    target_label = f"{label}.targets[{target_index}]"
                    for semantic, accessor_index in self.require_object(target, target_label).items():
                        self.check_index(accessor_index, accessors, f"{target_label}.{semantic}")

        for index, node in enumerate(self.document.get("nodes", [])):
            for field, collection in (
                ("mesh", meshes), ("skin", self.document.get("skins", [])),
                ("camera", self.document.get("cameras", [])),
            ):
                if field in node:
                    self.check_index(node[field], collection, f"nodes[{index}].{field}")
            for child in self.require_array(node.get("children", []), f"nodes[{index}].children"):
                self.check_index(child, self.document.get("nodes", []), f"nodes[{index}].children")

        scenes = self.document.get("scenes", [])
        for index, scene in enumerate(scenes):
            for node in self.require_array(scene.get("nodes", []), f"scenes[{index}].nodes"):
                self.check_index(node, self.document.get("nodes", []), f"scenes[{index}].nodes")
        if "scene" in self.document:
            self.check_index(self.document["scene"], scenes, "scene")

        for index, skin in enumerate(self.document.get("skins", [])):
            for field, collection in (
                ("inverseBindMatrices", accessors), ("skeleton", self.document.get("nodes", [])),
            ):
                if field in skin:
                    self.check_index(skin[field], collection, f"skins[{index}].{field}")
            for joint in self.require_array(skin.get("joints"), f"skins[{index}].joints"):
                self.check_index(joint, self.document.get("nodes", []), f"skins[{index}].joints")
        for index, animation in enumerate(self.document.get("animations", [])):
            label = f"animations[{index}]"
            samplers = self.require_array(animation.get("samplers"), f"{label}.samplers")
            for sampler_index, sampler in enumerate(samplers):
                sampler_label = f"{label}.samplers[{sampler_index}]"
                self.require_object(sampler, sampler_label)
                for field in ("input", "output"):
                    self.check_index(sampler.get(field), accessors, f"{sampler_label}.{field}")
            channels = self.require_array(animation.get("channels"), f"{label}.channels")
            for channel_index, channel in enumerate(channels):
                channel_label = f"{label}.channels[{channel_index}]"
                self.require_object(channel, channel_label)
                self.check_index(channel.get("sampler"), samplers, f"{channel_label}.sampler")
                target = self.require_object(channel.get("target"), f"{channel_label}.target")
                if "node" in target:
                    self.check_index(target["node"], self.document.get("nodes", []), f"{channel_label}.target.node")

    @staticmethod
    def parse_version(value: object) -> tuple[int, int]:
        if not isinstance(value, str) or not re.fullmatch(r"[0-9]+\.[0-9]+", value):
            raise GltfValidationError("asset version must be decimal major.minor")
        result = tuple(map(int, value.split(".")))
        if any(part > 2**64 - 1 for part in result):
            raise GltfValidationError("asset version component exceeds u64")
        return result

    def validate_version(self, asset: dict) -> None:
        version = self.parse_version(asset.get("version"))
        if version[0] != 2:
            raise GltfValidationError("asset.version must have major 2")
        if "minVersion" in asset:
            minimum = self.parse_version(asset["minVersion"])
            if minimum != (2, 0) or minimum > version:
                raise GltfValidationError("unsupported or inconsistent asset.minVersion")

    def validate_finite_json(self, value: object) -> None:
        # JSON 的 1e999 也会变成 inf，不能只拒绝 NaN/Infinity 字面量。
        pending = [value]
        while pending:
            value = pending.pop()
            if isinstance(value, float) and not math.isfinite(value):
                raise GltfValidationError("non-finite JSON number")
            if isinstance(value, dict):
                pending.extend(value.values())
            elif isinstance(value, list):
                pending.extend(value)

    def validate_extensions(self) -> None:
        used = self.require_array(self.document.get("extensionsUsed", []), "extensionsUsed")
        required = self.require_array(self.document.get("extensionsRequired", []), "extensionsRequired")
        supported = {"KHR_texture_transform", "KHR_materials_transmission", "KHR_materials_ior",
                     "KHR_materials_emissive_strength"}
        for values in (used, required):
            if any(not isinstance(value, str) for value in values) or len(set(values)) != len(values):
                raise GltfValidationError("extension lists must contain unique strings")
        if not set(required) <= set(used) or not set(required) <= supported:
            raise GltfValidationError("unsupported required extension or missing extensionsUsed declaration")

    def accessor_layout(self, index: int) -> tuple[str, int, int]:
        accessor = self.document["accessors"][index]
        formats = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
        widths = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT2": 4, "MAT3": 9, "MAT4": 16}
        try:
            component, width = formats[accessor["componentType"]], widths[accessor["type"]]
        except (KeyError, TypeError) as exc:
            raise GltfValidationError(f"accessors[{index}] invalid component/type") from exc
        # 小整数矩阵每列补齐到 4 字节；格式中的 x 只跳过 padding，不当成数据检查。
        size = struct.calcsize(component)
        if accessor["type"].startswith("MAT"):
            rows = int(accessor["type"][-1])
            column = component * rows + "x" * ((-rows * size) % 4)
            fmt = "<" + column * rows
        else:
            fmt = "<" + component * width
        count = accessor.get("count")
        if type(count) is not int or not 1 <= count <= 2**32 - 1:
            raise GltfValidationError(f"accessors[{index}] invalid count")
        return fmt, size, count

    def read_view(self, view_index: int, offset: int, count: int, fmt: str, alignment: int,
                  sparse: bool = False):
        view = self.document["bufferViews"][view_index]
        size = struct.calcsize(fmt)
        stride = view.get("byteStride", size)
        start = view.get("byteOffset", 0)
        if (type(offset) is not int or offset < 0 or type(stride) is not int or stride < size
                or offset % alignment or start % alignment or stride % alignment
                or offset + (count - 1) * stride + size > view["byteLength"]
                or ("byteStride" in view and (not 4 <= stride <= 252 or stride % 4))
                or (sparse and ("byteStride" in view or "target" in view))):
            raise GltfValidationError(f"bufferView {view_index} invalid accessor range/layout")
        data = self.buffers[view["buffer"]]
        for item in range(count):
            yield struct.unpack_from(fmt, data, start + offset + item * stride)

    def accessor_values(self, index: int):
        accessor = self.document["accessors"][index]
        fmt, alignment, count = self.accessor_layout(index)
        overrides = {}
        if "sparse" in accessor:
            sparse = accessor["sparse"]
            n = sparse.get("count")
            if type(n) is not int or not 1 <= n <= count:
                raise GltfValidationError(f"accessors[{index}] invalid sparse count")
            info = sparse["indices"]
            component = {5121: "B", 5123: "H", 5125: "I"}.get(info.get("componentType"))
            if component is None:
                raise GltfValidationError(f"accessors[{index}] invalid sparse index type")
            indices = self.read_view(info["bufferView"], info.get("byteOffset", 0), n,
                                     "<" + component, struct.calcsize(component), True)
            values = sparse["values"]
            values = self.read_view(values["bufferView"], values.get("byteOffset", 0), n, fmt, alignment, True)
            previous = -1
            for (slot,), value in zip(indices, values):
                if not previous < slot < count:
                    raise GltfValidationError(f"accessors[{index}] invalid sparse index order/range")
                overrides[slot] = value
                previous = slot
        if "bufferView" in accessor:
            base = self.read_view(accessor["bufferView"], accessor.get("byteOffset", 0), count, fmt, alignment)
        else:
            if "byteOffset" in accessor:
                raise GltfValidationError(f"accessors[{index}] offset without bufferView")
            zero = struct.unpack(fmt, bytes(struct.calcsize(fmt)))
            base = (zero for _ in range(count))
        for slot, value in enumerate(base):
            # 即使 sparse 覆盖旧值，原始浮点数据也必须有限，与 Rust Reader 一致。
            if any(not math.isfinite(v) for v in value):
                raise GltfValidationError(f"accessors[{index}] non-finite data")
            yield overrides.get(slot, value)

    def validate_accessors(self) -> None:
        for index, _ in enumerate(self.document.get("accessors", [])):
            for value in self.accessor_values(index):
                if any(not math.isfinite(v) for v in value):
                    raise GltfValidationError(f"accessors[{index}] non-finite data")

    def validate_semantics(self) -> None:
        accessors = self.document.get("accessors", [])
        for mi, material in enumerate(self.document.get("materials", [])):
            label = f"material {mi}"
            if material.get("alphaMode") == "BLEND":
                self.warnings.append(f"{label}: BLEND downgraded to Opaque")
            ext = self.require_object(material.get("extensions", {}), f"{label}.extensions")
            transmission = self.require_object(ext.get("KHR_materials_transmission", {}), label)
            if "transmissionTexture" in transmission:
                texture = self.require_object(transmission["transmissionTexture"], label)
                self.check_index(texture.get("index"), self.document.get("textures", []), label + ".transmissionTexture")
                self.warnings.append(f"{label}: transmission texture ignored; retaining factor")
            if transmission.get("transmissionFactor", 0) == 0 and "KHR_materials_ior" in ext:
                self.warnings.append(f"{label}: non-transmission IOR ignored")
        for mi, mesh in enumerate(self.document.get("meshes", [])):
            targets = None
            for pi, primitive in enumerate(mesh["primitives"]):
                label = f"mesh {mi} primitive {pi}"
                attributes = primitive["attributes"]
                if "POSITION" not in attributes:
                    raise GltfValidationError(f"{label}: missing POSITION")
                count = accessors[attributes["POSITION"]]["count"]
                if any(accessors[index]["count"] != count for index in attributes.values()):
                    raise GltfValidationError(f"{label}: inconsistent attribute lengths")
                if any(name.startswith("COLOR_") for name in attributes):
                    self.warnings.append(f"{label}: vertex colors ignored")
                morphs = primitive.get("targets", [])
                if targets is not None and targets != len(morphs):
                    raise GltfValidationError(f"mesh {mi}: inconsistent morph target counts")
                targets = len(morphs)
                for target in morphs:
                    if any(accessors[index]["count"] != count for index in target.values()):
                        raise GltfValidationError(f"{label}: inconsistent morph lengths")
                if "indices" in primitive:
                    index = primitive["indices"]
                    accessor = accessors[index]
                    component = accessor["componentType"]
                    if accessor["type"] != "SCALAR" or component not in (5121, 5123, 5125):
                        raise GltfValidationError(f"{label}: invalid index accessor")
                    restart = {5121: 255, 5123: 65535, 5125: 2**32 - 1}[component]
                    for (value,) in self.accessor_values(index):
                        if value >= count or value == restart:
                            raise GltfValidationError(f"{label}: index out of range/restart")
                    count = accessor["count"]
                if primitive.get("mode", 4) == 4 and count % 3:
                    raise GltfValidationError(f"{label}: incomplete triangles")
            if targets:
                self.warnings.append(f"mesh {mi}: morph deformation ignored; using base mesh")
            if "weights" in mesh and len(self.require_array(mesh["weights"], "mesh weights")) != targets:
                raise GltfValidationError(f"mesh {mi}: inconsistent morph weights")
        for ni, node in enumerate(self.document.get("nodes", [])):
            if "skin" in node:
                self.warnings.append(f"node {ni}: skin ignored; unskinned base mesh + existing node transform (not bind pose)")
            if "weights" in node and "mesh" in node:
                mesh = self.document["meshes"][node["mesh"]]
                count = len(mesh["primitives"][0].get("targets", [])) if mesh["primitives"] else 0
                if len(self.require_array(node["weights"], "node weights")) != count:
                    raise GltfValidationError(f"node {ni}: inconsistent morph weights")
        for index, _ in enumerate(self.document.get("animations", [])):
            self.warnings.append(f"animation {index}: ignored; using existing node transforms")

    def check_index(self, value: object, collection: list, label: str) -> bool:
        if type(value) is not int or value < 0 or value >= len(collection):
            self.errors.append(f"{label} has invalid index {value}")
            return False
        return True

    def check_uint(self, value: object, label: str, minimum: int = 0) -> bool:
        if type(value) is not int or value < minimum:
            self.errors.append(f"{label} must be an integer >= {minimum}")
            return False
        return True

    def summary(self) -> str:
        return "".join(f"Warning: {warning}\n" for warning in self.warnings) + (
            f"Resource/reference preflight passed for {self.scene}: "
            f"nodes={len(self.document.get('nodes', []))} "
            f"meshes={len(self.document.get('meshes', []))} "
            f"materials={len(self.document.get('materials', []))} "
            f"images={len(self.document.get('images', []))}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scene", type=Path, required=True)
    args = parser.parse_args()
    validator = GltfValidator(args.scene)
    try:
        validator.validate()
    except GltfValidationError as exc:
        print(f"glTF validation failed: {exc}", file=sys.stderr)
        return 1
    print(validator.summary())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
