# Explainer Image Spec

The renderer accepts a JSON object through `--spec`. `--spec` may be a JSON string or a path to a JSON file.

## Top-Level Fields

```json
{
  "title": "ReSTIR DI 的直觉",
  "subtitle": "用候选复用降低直接光噪声",
  "slug": "restir-di",
  "pages": []
}
```

- `title`: Required fallback title.
- `subtitle`: Optional fallback subtitle.
- `slug`: Optional output file prefix. If omitted, the renderer derives one from `title`.
- `pages`: Required array. Use 1-4 pages for most tasks.

## Page Fields

```json
{
  "kind": "workflow",
  "title": "四阶段流程",
  "subtitle": "Path / Temporal / Spatial / Final",
  "cards": [],
  "nodes": [],
  "edges": [],
  "callouts": [],
  "formula_blocks": []
}
```

`kind` can be:

- `overview`: Cards plus small schematic elements for one central idea.
- `workflow`: Ordered stages connected left-to-right.
- `comparison`: Two or more cards comparing approaches.
- `dataflow`: Node graph for data movement.
- `invariant`: Rules, formulas, and correct/incorrect callouts.
- `module-map`: Module ownership, dependencies, public interfaces, and responsibilities.

## Cards

```json
{
  "title": "普通 NEE",
  "body": "每个像素各自抽一个光源样本。",
  "bullets": ["简单", "高亮小灯容易产生噪点"],
  "accent": "red",
  "role": "wrong"
}
```

Cards are auto-laid out unless `x`, `y`, `w`, and `h` are provided in normalized coordinates from `0.0` to `1.0`.

## Nodes and Edges

```json
{
  "id": "temporal",
  "label": "Temporal reuse",
  "detail": "motion vector 回投上一帧",
  "accent": "blue"
}
```

```json
{
  "from": "path",
  "to": "temporal",
  "label": "image barrier"
}
```

Use nodes and edges for workflows, data flow, and module maps. If node positions are omitted, the renderer chooses a simple layout based on page kind.

## Callouts

```json
{
  "text": "final reservoir 不回灌 temporal history",
  "accent": "amber"
}
```

Callouts emphasize constraints, risks, or takeaways. Keep them concise.

## Formula Blocks

```json
{
  "title": "Reservoir shade weight",
  "formula": "W = weight_sum / (target(selected) * M)",
  "note": "最终 RGB contribution 仍单独计算。"
}
```

Use formula blocks only for formulas that directly clarify the visual explanation.
