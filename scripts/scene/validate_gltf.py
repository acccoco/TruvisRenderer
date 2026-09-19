"""校验独立 glTF 场景及其外部资源。"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from urllib.parse import unquote


class GltfValidationError(RuntimeError):
    pass


class GltfValidator:
    def __init__(self, scene: Path) -> None:
        self.scene = scene.resolve()
        self.root = self.scene.parent
        self.document: dict = {}
        self.errors: list[str] = []

    def validate(self) -> None:
        try:
            self.document = json.loads(self.scene.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise GltfValidationError(f"cannot read glTF JSON: {exc}") from exc
        if not isinstance(self.document, dict):
            raise GltfValidationError("glTF root must be an object")

        self.validate_external_resources()
        self.validate_references()
        if self.errors:
            raise GltfValidationError("\n".join(self.errors))

    def validate_external_resources(self) -> None:
        for kind in ("buffers", "images"):
            for index, item in enumerate(self.document.get(kind, [])):
                if not isinstance(item, dict):
                    self.errors.append(f"{kind}[{index}] must be an object")
                    continue
                uri = item.get("uri")
                if not isinstance(uri, str):
                    self.errors.append(f"{kind}[{index}] must use an external URI")
                    continue
                if uri.startswith("data:"):
                    self.errors.append(f"{kind}[{index}] uses an embedded data URI")
                    continue
                resolved = self.resolve_uri(uri)
                if resolved is None:
                    continue
                if not resolved.is_file():
                    self.errors.append(f"missing {kind}[{index}] URI: {uri}")

    def resolve_uri(self, uri: str) -> Path | None:
        decoded = unquote(uri)
        candidate = (self.root / decoded).resolve()
        try:
            candidate.relative_to(self.root)
        except ValueError:
            self.errors.append(f"URI escapes scene directory: {uri}")
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
            if isinstance(accessor, dict) and "bufferView" in accessor:
                self.check_index(accessor["bufferView"], buffer_views, f"accessors[{index}].bufferView")
        for index, view in enumerate(buffer_views):
            if isinstance(view, dict):
                self.check_index(view.get("buffer"), buffers, f"bufferViews[{index}].buffer")
        for index, texture in enumerate(textures):
            if isinstance(texture, dict) and "source" in texture:
                self.check_index(texture["source"], images, f"textures[{index}].source")
            if isinstance(texture, dict) and "sampler" in texture:
                self.check_index(
                    texture["sampler"],
                    self.document.get("samplers", []),
                    f"textures[{index}].sampler",
                )
        for index, material in enumerate(materials):
            if not isinstance(material, dict):
                self.errors.append(f"materials[{index}] must be an object")
                continue
            pbr = material.get("pbrMetallicRoughness", {})
            for label, info in (
                ("baseColorTexture", pbr.get("baseColorTexture")),
                ("metallicRoughnessTexture", pbr.get("metallicRoughnessTexture")),
                ("normalTexture", material.get("normalTexture")),
                ("occlusionTexture", material.get("occlusionTexture")),
                ("emissiveTexture", material.get("emissiveTexture")),
            ):
                if isinstance(info, dict) and "index" in info:
                    self.check_index(info["index"], textures, f"materials[{index}].{label}")
        for index, mesh in enumerate(meshes):
            if not isinstance(mesh, dict):
                self.errors.append(f"meshes[{index}] must be an object")
                continue
            for primitive_index, primitive in enumerate(mesh.get("primitives", [])):
                if not isinstance(primitive, dict):
                    self.errors.append(f"meshes[{index}].primitives[{primitive_index}] must be an object")
                    continue
                for semantic, accessor_index in primitive.get("attributes", {}).items():
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

        for index, node in enumerate(self.document.get("nodes", [])):
            if not isinstance(node, dict):
                self.errors.append(f"nodes[{index}] must be an object")
                continue
            if "mesh" in node:
                self.check_index(node["mesh"], meshes, f"nodes[{index}].mesh")
            for child in node.get("children", []):
                self.check_index(child, self.document.get("nodes", []), f"nodes[{index}].children")

        scenes = self.document.get("scenes", [])
        for index, scene in enumerate(scenes):
            if not isinstance(scene, dict):
                self.errors.append(f"scenes[{index}] must be an object")
                continue
            for node in scene.get("nodes", []):
                self.check_index(node, self.document.get("nodes", []), f"scenes[{index}].nodes")
        if "scene" in self.document:
            self.check_index(self.document["scene"], scenes, "scene")

    def check_index(self, value: object, collection: list, label: str) -> None:
        if not isinstance(value, int) or value < 0 or value >= len(collection):
            self.errors.append(f"{label} has invalid index {value}")

    def summary(self) -> str:
        return (
            f"Validated {self.scene}: "
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
