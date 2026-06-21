# Visual Style

## Purpose

Create teaching images that make a theory, algorithm, workflow, or module easy to understand at a glance. The diagram should be useful in documentation or slides without requiring a long spoken explanation.

## Layout

- Use 16:9 by default.
- Put one main idea on each page. Split pages when the canvas needs more than 5 major blocks or when two unrelated explanations compete.
- Use a clear title and one short subtitle.
- Prefer full-width bands, cards, workflow lanes, or node graphs. Avoid nested cards.
- Leave enough whitespace around text and arrows. Dense technical content should be split across pages.

## Text

- Render all important text with the Python/Pillow renderer.
- Keep labels short. Use callouts for details instead of long labels inside small nodes.
- For Chinese output, prefer mixed Chinese plus stable English technical terms, such as `surface key`, `reservoir`, `RenderGraph`.
- Verify text visually after rendering; automatic wrapping is a helper, not a substitute for inspection.

## Color

- Use color as semantics, not decoration.
- Suggested mappings:
  - Blue: history, previous frame, persisted state.
  - Teal/green: current valid flow, accepted result, correct path.
  - Amber: local/current sampling, proposal, input.
  - Red/coral: rejection, error, wrong reuse, risk.
  - Purple: final output, mode switch, user-facing result.
- Keep backgrounds light and cards white or lightly tinted.

## Validation

Before delivery, inspect each generated image and check:

- No text overlaps with icons, nodes, arrows, or other text.
- Every arrow has an obvious source and destination.
- The image explains the requested theory/module, not merely names its parts.
- Any formula has a short explanation of what it means.
- File names and paths match the requested output location.
