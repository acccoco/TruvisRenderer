# Workspace scripts

`scripts/` contains offline tooling only. These scripts read source scenes or
build metadata and produce conversion/build outputs; runtime Rust crates do not
depend on them.

## Scene conversion

`scene/export_gltf.py` accepts a `.blend` or `.fbx` source and writes a separate
glTF scene with external textures. Python 3 and Blender are required; Pillow is
required when the source texture root contains images that must be staged or
converted:

```text
python scripts/scene/export_gltf.py \
  --source <scene.blend|scene.fbx> \
  --output-dir <output-directory> \
  --blender <blender-executable>
```

The output contains `scene.gltf` and `textures/`. The source scene and source
textures are read-only. Blender is required only during conversion; the runtime
loads the resulting glTF and external images through the existing asset path.

`scene/validate_gltf.py` checks glTF references and external buffer/image paths:

```text
python scripts/scene/validate_gltf.py --scene <output-directory>/scene.gltf
```

The converter intentionally has no Bistro-specific scene or material names and
does not emit a manifest or `export.json`.

## Nushell helpers

The `nu/` scripts hold imperative environment setup previously embedded in
`justfile`. Public recipes remain in `justfile`; the helper scripts receive the
repository root and explicit arguments.
