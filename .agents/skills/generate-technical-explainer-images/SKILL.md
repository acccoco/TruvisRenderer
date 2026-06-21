---
name: generate-technical-explainer-images
description: Generate PNG/JPG technical explainer diagrams with stable rendered text, schematic graphics, workflow/data-flow/module maps, comparison cards, callouts, and formula blocks. Use when Codex needs to explain a theory, algorithm, rendering workflow, system module, code architecture, or project implementation as one or more image files rather than Mermaid or plain text.
---

# Generate Technical Explainer Images

## Workflow

1. Clarify the teaching goal, audience, and output directory. If the topic is a project module, read the relevant source, `AGENTS.md`, `docs/ARCHITECTURE.md`, related `docs/summaries/`, and nearby module docs before designing the images. If the topic is a theory, use authoritative sources or user-provided material.
2. Choose the smallest number of pages that explains the topic clearly. Use one page for a simple idea; use multiple pages only when a single canvas would become dense or mix unrelated concepts.
3. Draft a JSON spec before rendering. Use `references/spec-schema.md` for fields and page kinds, and `references/visual-style.md` for layout rules.
4. Run `scripts/render_explainer_images.py --output-dir <dir> --spec <json-or-path>`. Prefer PNG unless the user asks for JPG.
5. Inspect every generated image with `view_image`. Iterate on the spec if text overlaps, arrows are ambiguous, or the page has too many concepts.
6. Return absolute file links and Markdown image previews.

## Renderer

Use the bundled renderer for deterministic text and layout:

```powershell
python .agents\skills\generate-technical-explainer-images\scripts\render_explainer_images.py `
  --output-dir docs\imgs\my-topic `
  --spec '{"title":"...","subtitle":"...","pages":[...]}'
```

`--spec` accepts either a JSON string or a path to a JSON file. The renderer outputs files named `<slug>-01.png`, `<slug>-02.png`, etc.

## Design Rules

- Make each page teach one main point.
- Use text labels rendered by the script. Do not rely on AI image generation for readable text.
- Prefer schematic shapes, cards, arrows, grids, and callouts over decorative illustration.
- Map color to meaning, such as current/history/neighbor, input/process/output, or correct/incorrect.
- Keep formulas short and paired with plain-language interpretation.

## References

- Read `references/spec-schema.md` when creating or modifying a JSON spec.
- Read `references/visual-style.md` when choosing page count, visual hierarchy, colors, and validation criteria.
