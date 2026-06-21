#!/usr/bin/env python3
"""Render technical explainer images from a compact JSON spec."""

from __future__ import annotations

import argparse
import json
import math
import re
import textwrap
from pathlib import Path
from typing import Any

from PIL import Image, ImageDraw, ImageFilter, ImageFont


PALETTE = {
    "bg": "#F7F8FA",
    "ink": "#17202A",
    "muted": "#5D6875",
    "line": "#C9D1DA",
    "white": "#FFFFFF",
    "dark": "#243447",
    "blue": "#2F80ED",
    "blue2": "#DCEBFF",
    "teal": "#00A896",
    "teal2": "#D9F3EF",
    "green": "#2E7D32",
    "green2": "#E2F3E3",
    "amber": "#F2A93B",
    "amber2": "#FFF1D5",
    "purple": "#7A5AF8",
    "purple2": "#E8E2FF",
    "red": "#C62828",
    "red2": "#FFE5E5",
    "coral": "#E85D75",
    "coral2": "#FFE1E7",
}

ACCENT_FALLBACK = "blue"


def parse_size(value: str) -> tuple[int, int]:
    match = re.fullmatch(r"(\d+)x(\d+)", value.strip().lower())
    if not match:
        raise ValueError("--size must use WIDTHxHEIGHT, for example 1920x1080")
    width, height = int(match.group(1)), int(match.group(2))
    if width < 800 or height < 450:
        raise ValueError("--size is too small for readable explainer images")
    return width, height


def load_spec(value: str) -> dict[str, Any]:
    stripped = value.strip()
    if stripped.startswith("{"):
        return json.loads(stripped)
    path = Path(value)
    if path.exists():
        return json.loads(path.read_text(encoding="utf-8"))
    return json.loads(stripped)


def slugify(value: str) -> str:
    ascii_slug = re.sub(r"[^a-zA-Z0-9]+", "-", value).strip("-").lower()
    return ascii_slug or "explainer"


def color(name: str | None, light: bool = False) -> str:
    key = name or ACCENT_FALLBACK
    if light and f"{key}2" in PALETTE:
        return PALETTE[f"{key}2"]
    return PALETTE.get(key, PALETTE[ACCENT_FALLBACK])


class ExplainerRenderer:
    """把 spec 翻译成稳定图片；所有绘图状态收敛在该类型里，避免布局参数散落。"""

    def __init__(self, width: int, height: int, output_format: str) -> None:
        self.width = width
        self.height = height
        self.scale = 2
        self.output_format = output_format.lower()
        self.fonts = self._load_fonts()

    def render(self, spec: dict[str, Any], output_dir: Path) -> list[Path]:
        pages = spec.get("pages")
        if not isinstance(pages, list) or not pages:
            raise ValueError("spec.pages must be a non-empty array")
        output_dir.mkdir(parents=True, exist_ok=True)
        slug = slugify(str(spec.get("slug") or spec.get("title") or "explainer"))
        paths: list[Path] = []
        for index, page in enumerate(pages, start=1):
            image = self._render_page(spec, page)
            ext = "jpg" if self.output_format in {"jpg", "jpeg"} else "png"
            path = output_dir / f"{slug}-{index:02d}.{ext}"
            if ext == "jpg":
                image.save(path, "JPEG", quality=94, optimize=True)
            else:
                image.save(path, "PNG", optimize=True)
            paths.append(path)
        return paths

    def _load_fonts(self) -> dict[str, ImageFont.FreeTypeFont]:
        candidates = [
            Path(r"C:\Windows\Fonts\NotoSansSC-VF.ttf"),
            Path(r"C:\Windows\Fonts\simhei.ttf"),
            Path(r"C:\Windows\Fonts\msyh.ttc"),
            Path("/System/Library/Fonts/PingFang.ttc"),
            Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        ]
        font_path = next((p for p in candidates if p.exists()), None)
        mono_candidates = [
            Path(r"C:\Windows\Fonts\SourceCodePro-Regular-12.ttf"),
            Path(r"C:\Windows\Fonts\consola.ttf"),
            Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
        ]
        mono_path = next((p for p in mono_candidates if p.exists()), font_path)
        if font_path is None:
            raise RuntimeError("No usable TrueType font found")
        return {
            "title": ImageFont.truetype(str(font_path), self._s(46)),
            "subtitle": ImageFont.truetype(str(font_path), self._s(25)),
            "h1": ImageFont.truetype(str(font_path), self._s(34)),
            "h2": ImageFont.truetype(str(font_path), self._s(27)),
            "body": ImageFont.truetype(str(font_path), self._s(22)),
            "small": ImageFont.truetype(str(font_path), self._s(18)),
            "tiny": ImageFont.truetype(str(font_path), self._s(15)),
            "mono": ImageFont.truetype(str(mono_path), self._s(19)),
        }

    def _render_page(self, spec: dict[str, Any], page: dict[str, Any]) -> Image.Image:
        image = Image.new("RGB", (self.width * self.scale, self.height * self.scale), PALETTE["bg"])
        draw = ImageDraw.Draw(image)
        title = str(page.get("title") or spec.get("title") or "Technical Explainer")
        subtitle = str(page.get("subtitle") or spec.get("subtitle") or "")
        self._text(draw, 70, 48, title, "title", PALETTE["ink"], max_width=self.width - 140)
        if subtitle:
            self._text(draw, 72, 112, subtitle, "subtitle", PALETTE["muted"], max_width=self.width - 144)

        content_top = 180
        content_bottom = self.height - 70
        kind = str(page.get("kind") or "overview")
        if kind == "workflow":
            self._draw_workflow(image, draw, page, content_top, content_bottom)
        elif kind in {"dataflow", "module-map"}:
            self._draw_graph(image, draw, page, content_top, content_bottom, module_map=(kind == "module-map"))
        elif kind == "comparison":
            self._draw_comparison(image, draw, page, content_top, content_bottom)
        elif kind == "invariant":
            self._draw_invariant(image, draw, page, content_top, content_bottom)
        else:
            self._draw_overview(image, draw, page, content_top, content_bottom)

        return image.resize((self.width, self.height), Image.Resampling.LANCZOS).filter(
            ImageFilter.UnsharpMask(radius=1.0, percent=110, threshold=2)
        )

    def _draw_overview(self, image: Image.Image, draw: ImageDraw.ImageDraw, page: dict[str, Any], top: int, bottom: int) -> None:
        cards = self._items(page, "cards")
        if not cards:
            cards = [{"title": "Core idea", "body": page.get("body", ""), "accent": "blue"}]
        self._draw_cards(image, draw, cards, top, bottom - 145)
        self._draw_bottom_extras(image, draw, page, bottom - 120, bottom)

    def _draw_comparison(self, image: Image.Image, draw: ImageDraw.ImageDraw, page: dict[str, Any], top: int, bottom: int) -> None:
        cards = self._items(page, "cards")
        self._draw_cards(image, draw, cards, top, bottom - 70)
        for index in range(max(0, len(cards) - 1)):
            left = self._auto_card_box(index, len(cards), top, bottom - 70)
            right = self._auto_card_box(index + 1, len(cards), top, bottom - 70)
            self._arrow(draw, (left[2] + 16, (left[1] + left[3]) / 2), (right[0] - 16, (right[1] + right[3]) / 2))

    def _draw_workflow(self, image: Image.Image, draw: ImageDraw.ImageDraw, page: dict[str, Any], top: int, bottom: int) -> None:
        nodes = self._items(page, "nodes") or self._items(page, "cards")
        if not nodes:
            nodes = [{"label": "Step 1", "detail": "Add workflow nodes to the spec."}]
        count = len(nodes)
        gap = 32
        usable_width = self.width - 140
        box_width = max(260, (usable_width - gap * (count - 1)) / count)
        box_height = bottom - top - 135
        y = top + 10
        centers: list[tuple[float, float]] = []
        for index, node in enumerate(nodes):
            x = 70 + index * (box_width + gap)
            box = (x, y, x + box_width, y + box_height)
            accent = str(node.get("accent") or self._accent_for_index(index))
            self._card(image, draw, box)
            self._dot(draw, x + 42, y + 42, 22, color(accent))
            self._text(draw, x + 34, y + 25, str(index + 1), "h2", PALETTE["white"])
            self._text(draw, x + 82, y + 26, str(node.get("label") or node.get("title") or f"Step {index + 1}"), "h2", color(accent), max_width=box_width - 108)
            detail = node.get("detail") or node.get("body") or ""
            if detail:
                self._text(draw, x + 82, y + 66, str(detail), "small", PALETTE["muted"], max_width=box_width - 108)
            self._bullets(draw, node.get("bullets", []), x + 36, y + 135, box_width - 72, accent)
            output = node.get("output")
            if output:
                self._round_rect(draw, (x + 24, y + box_height - 72, x + box_width - 24, y + box_height - 24), color(accent, True), color(accent), 12)
                self._text(draw, x + 44, y + box_height - 58, str(output), "small", PALETTE["ink"], max_width=box_width - 88)
            centers.append((x + box_width, y + box_height / 2))
            if index > 0:
                prev = centers[index - 1]
                self._arrow(draw, (prev[0] + 10, prev[1]), (x - 12, prev[1]))
        self._draw_bottom_extras(image, draw, page, bottom - 105, bottom)

    def _draw_graph(self, image: Image.Image, draw: ImageDraw.ImageDraw, page: dict[str, Any], top: int, bottom: int, module_map: bool) -> None:
        nodes = self._items(page, "nodes")
        if not nodes:
            nodes = [{"id": "a", "label": "Input"}, {"id": "b", "label": "Process"}, {"id": "c", "label": "Output"}]
        positions = self._graph_positions(nodes, top, bottom)
        node_by_id = {str(node.get("id") or index): node for index, node in enumerate(nodes)}
        for edge in self._items(page, "edges"):
            src_id = str(edge.get("from"))
            dst_id = str(edge.get("to"))
            if src_id not in positions or dst_id not in positions:
                continue
            src = positions[src_id]
            dst = positions[dst_id]
            self._arrow(draw, (src[2], (src[1] + src[3]) / 2), (dst[0], (dst[1] + dst[3]) / 2), fill=PALETTE["dark"], width=4)
            label = edge.get("label")
            if label:
                self._text(draw, (src[2] + dst[0]) / 2 - 70, (src[1] + dst[1]) / 2 - 28, str(label), "tiny", PALETTE["muted"], max_width=140, align="center")
        for node_id, box in positions.items():
            node = node_by_id[node_id]
            accent = str(node.get("accent") or ("purple" if module_map else "blue"))
            self._card(image, draw, box, fill=color(accent, True), outline=color(accent))
            self._text(draw, box[0] + 24, box[1] + 24, str(node.get("label") or node_id), "h2", color(accent), max_width=box[2] - box[0] - 48)
            if node.get("detail"):
                self._text(draw, box[0] + 24, box[1] + 72, str(node["detail"]), "body", PALETTE["ink"], max_width=box[2] - box[0] - 48)
            self._bullets(draw, node.get("bullets", []), box[0] + 30, box[1] + 132, box[2] - box[0] - 60, accent)
        self._draw_bottom_extras(image, draw, page, bottom - 115, bottom)

    def _draw_invariant(self, image: Image.Image, draw: ImageDraw.ImageDraw, page: dict[str, Any], top: int, bottom: int) -> None:
        cards = self._items(page, "cards")
        formulas = self._items(page, "formula_blocks")
        callouts = self._items(page, "callouts")
        top_height = 360 if formulas else 0
        if formulas:
            self._draw_formula_blocks(image, draw, formulas, top, top + top_height)
        content_top = top + top_height + 34 if formulas else top
        if cards:
            self._draw_cards(image, draw, cards, content_top, bottom - 100)
        if callouts:
            self._draw_callouts(image, draw, callouts, bottom - 92, bottom - 18)

    def _draw_cards(self, image: Image.Image, draw: ImageDraw.ImageDraw, cards: list[dict[str, Any]], top: int, bottom: int) -> None:
        for index, item in enumerate(cards):
            box = self._box_from_item(item) or self._auto_card_box(index, len(cards), top, bottom)
            accent = str(item.get("accent") or self._accent_for_index(index))
            self._card(image, draw, box)
            self._text(draw, box[0] + 36, box[1] + 34, str(item.get("title") or item.get("label") or "Card"), "h1", color(accent), max_width=box[2] - box[0] - 72)
            cursor = box[1] + 98
            if item.get("body"):
                cursor += self._text(draw, box[0] + 36, cursor, str(item["body"]), "body", PALETTE["ink"], max_width=box[2] - box[0] - 72) + 18
            if item.get("bullets"):
                self._bullets(draw, item["bullets"], box[0] + 44, cursor, box[2] - box[0] - 88, accent)
            if item.get("tag"):
                self._round_rect(draw, (box[0] + 36, box[3] - 68, box[2] - 36, box[3] - 24), color(accent, True), color(accent), 22)
                self._text(draw, box[0] + 54, box[3] - 57, str(item["tag"]), "small", color(accent), max_width=box[2] - box[0] - 108, align="center")

    def _draw_formula_blocks(self, image: Image.Image, draw: ImageDraw.ImageDraw, formulas: list[dict[str, Any]], top: int, bottom: int) -> None:
        for index, item in enumerate(formulas):
            box = self._auto_card_box(index, len(formulas), top, bottom)
            accent = str(item.get("accent") or self._accent_for_index(index))
            self._card(image, draw, box)
            self._text(draw, box[0] + 36, box[1] + 30, str(item.get("title") or "Formula"), "h2", color(accent), max_width=box[2] - box[0] - 72)
            self._text(draw, box[0] + 36, box[1] + 96, str(item.get("formula") or ""), "mono", PALETTE["ink"], max_width=box[2] - box[0] - 72)
            if item.get("note"):
                self._text(draw, box[0] + 36, box[1] + 188, str(item["note"]), "body", PALETTE["muted"], max_width=box[2] - box[0] - 72)

    def _draw_bottom_extras(self, image: Image.Image, draw: ImageDraw.ImageDraw, page: dict[str, Any], top: int, bottom: int) -> None:
        formulas = self._items(page, "formula_blocks")
        callouts = self._items(page, "callouts")
        extras: list[dict[str, Any]] = []
        for formula in formulas:
            extras.append({"text": f"{formula.get('title', 'Formula')}: {formula.get('formula', '')}", "accent": formula.get("accent", "purple")})
        extras.extend(callouts)
        if extras:
            self._draw_callouts(image, draw, extras[:4], top, bottom)

    def _draw_callouts(self, image: Image.Image, draw: ImageDraw.ImageDraw, callouts: list[dict[str, Any]], top: int, bottom: int) -> None:
        count = len(callouts)
        if count == 0:
            return
        gap = 24
        box_width = (self.width - 140 - gap * (count - 1)) / count
        for index, item in enumerate(callouts):
            accent = str(item.get("accent") or self._accent_for_index(index))
            box = (70 + index * (box_width + gap), top, 70 + index * (box_width + gap) + box_width, bottom)
            self._round_rect(draw, box, color(accent, True), color(accent), 18)
            self._text(draw, box[0] + 22, box[1] + 18, str(item.get("text") or item.get("body") or ""), "small", color(accent), max_width=box_width - 44, align="center")

    def _graph_positions(self, nodes: list[dict[str, Any]], top: int, bottom: int) -> dict[str, tuple[float, float, float, float]]:
        positions: dict[str, tuple[float, float, float, float]] = {}
        count = len(nodes)
        cols = 3 if count > 4 else max(1, count)
        rows = math.ceil(count / cols)
        gap_x = 46
        gap_y = 42
        box_w = (self.width - 140 - gap_x * (cols - 1)) / cols
        graph_top = top + 45
        graph_bottom = bottom - 165
        box_h = min(230, (graph_bottom - graph_top - gap_y * (rows - 1)) / rows)
        total_h = box_h * rows + gap_y * (rows - 1)
        start_y = graph_top + max(0, (graph_bottom - graph_top - total_h) / 2)
        for index, node in enumerate(nodes):
            explicit = self._box_from_item(node)
            node_id = str(node.get("id") or index)
            if explicit:
                positions[node_id] = explicit
                continue
            row = index // cols
            col = index % cols
            x = 70 + col * (box_w + gap_x)
            y = start_y + row * (box_h + gap_y)
            positions[node_id] = (x, y, x + box_w, y + box_h)
        return positions

    def _auto_card_box(self, index: int, count: int, top: int, bottom: int) -> tuple[float, float, float, float]:
        count = max(count, 1)
        if count <= 2:
            cols = count
        elif count <= 4:
            cols = 2
        else:
            cols = 3
        rows = math.ceil(count / cols)
        gap_x = 40
        gap_y = 38
        box_w = (self.width - 140 - gap_x * (cols - 1)) / cols
        box_h = (bottom - top - gap_y * (rows - 1)) / rows
        row = index // cols
        col = index % cols
        x = 70 + col * (box_w + gap_x)
        y = top + row * (box_h + gap_y)
        return (x, y, x + box_w, y + box_h)

    def _box_from_item(self, item: dict[str, Any]) -> tuple[float, float, float, float] | None:
        required = ("x", "y", "w", "h")
        if not all(key in item for key in required):
            return None
        return (
            float(item["x"]) * self.width,
            float(item["y"]) * self.height,
            (float(item["x"]) + float(item["w"])) * self.width,
            (float(item["y"]) + float(item["h"])) * self.height,
        )

    def _items(self, page: dict[str, Any], key: str) -> list[dict[str, Any]]:
        value = page.get(key) or []
        if not isinstance(value, list):
            raise ValueError(f"page.{key} must be an array")
        return [item for item in value if isinstance(item, dict)]

    def _bullets(self, draw: ImageDraw.ImageDraw, bullets: list[Any], x: float, y: float, max_width: float, accent: str) -> None:
        cursor = y
        for bullet in bullets:
            self._dot(draw, x, cursor + 14, 5, color(accent))
            used = self._text(draw, x + 24, cursor, str(bullet), "body", PALETTE["ink"], max_width=max_width - 24)
            cursor += max(48, used + 20)

    def _card(self, image: Image.Image, draw: ImageDraw.ImageDraw, box: tuple[float, float, float, float], fill: str = "#FFFFFF", outline: str = "#D9E0E7") -> None:
        shadow = Image.new("RGBA", image.size, (0, 0, 0, 0))
        shadow_draw = ImageDraw.Draw(shadow)
        shadow_draw.rounded_rectangle(self._xy((box[0] + 0, box[1] + 8, box[2] + 0, box[3] + 8)), radius=self._s(18), fill=(10, 20, 35, 28))
        shadow = shadow.filter(ImageFilter.GaussianBlur(self._s(18)))
        image.paste(Image.alpha_composite(image.convert("RGBA"), shadow).convert("RGB"))
        self._round_rect(draw, box, fill, outline, 18)

    def _round_rect(self, draw: ImageDraw.ImageDraw, box: tuple[float, float, float, float], fill: str, outline: str | None = None, radius: float = 18) -> None:
        draw.rounded_rectangle(self._xy(box), radius=self._s(radius), fill=fill, outline=outline, width=self._s(1))

    def _arrow(self, draw: ImageDraw.ImageDraw, start: tuple[float, float], end: tuple[float, float], fill: str = PALETTE["dark"], width: int = 4, head: int = 16) -> None:
        x1, y1, x2, y2 = map(self._s, [start[0], start[1], end[0], end[1]])
        draw.line((x1, y1, x2, y2), fill=fill, width=self._s(width))
        angle = math.atan2(y2 - y1, x2 - x1)
        h = self._s(head)
        points = [
            (x2, y2),
            (x2 - h * math.cos(angle - math.pi / 6), y2 - h * math.sin(angle - math.pi / 6)),
            (x2 - h * math.cos(angle + math.pi / 6), y2 - h * math.sin(angle + math.pi / 6)),
        ]
        draw.polygon(points, fill=fill)

    def _dot(self, draw: ImageDraw.ImageDraw, x: float, y: float, radius: float, fill: str) -> None:
        draw.ellipse(self._xy((x - radius, y - radius, x + radius, y + radius)), fill=fill, outline=PALETTE["white"], width=self._s(2))

    def _text(
        self,
        draw: ImageDraw.ImageDraw,
        x: float,
        y: float,
        text: str,
        style: str,
        fill: str,
        max_width: float | None = None,
        align: str = "left",
    ) -> float:
        font = self.fonts[style]
        if max_width is None:
            draw.text((self._s(x), self._s(y)), text, font=font, fill=fill)
            return self._text_size(draw, text, font)[1] / self.scale
        lines = self._wrap(draw, text, font, self._s(max_width))
        ascent, descent = font.getmetrics()
        line_height = ascent + descent + self._s(6)
        cursor = self._s(y)
        for line in lines:
            line_width, _ = self._text_size(draw, line, font)
            offset = (self._s(max_width) - line_width) / 2 if align == "center" else 0
            draw.text((self._s(x) + offset, cursor), line, font=font, fill=fill)
            cursor += line_height
        return (len(lines) * line_height) / self.scale

    def _wrap(self, draw: ImageDraw.ImageDraw, text: str, font: ImageFont.FreeTypeFont, max_width: int) -> list[str]:
        lines: list[str] = []
        for paragraph in str(text).splitlines() or [""]:
            if not paragraph:
                lines.append("")
                continue
            tokens = self._tokenize(paragraph)
            current = ""
            for token in tokens:
                candidate = current + token
                if not current or self._text_size(draw, candidate, font)[0] <= max_width:
                    current = candidate
                else:
                    lines.append(current.rstrip())
                    current = token.lstrip()
            if current:
                lines.append(current.rstrip())
        return lines

    def _tokenize(self, text: str) -> list[str]:
        tokens: list[str] = []
        for part in re.findall(r"[A-Za-z0-9_./:*+\-=<>|()]+|\s+|.", text):
            if part.isspace():
                tokens.append(" ")
            elif len(part) > 28 and re.fullmatch(r"[A-Za-z0-9_./:*+\-=<>|()]+", part):
                tokens.extend(textwrap.wrap(part, 24, break_long_words=True, break_on_hyphens=False))
            else:
                tokens.append(part)
        return tokens

    def _text_size(self, draw: ImageDraw.ImageDraw, text: str, font: ImageFont.FreeTypeFont) -> tuple[int, int]:
        box = draw.textbbox((0, 0), text, font=font)
        return box[2] - box[0], box[3] - box[1]

    def _accent_for_index(self, index: int) -> str:
        return ["amber", "blue", "teal", "purple", "red", "green"][index % 6]

    def _xy(self, box: tuple[float, float, float, float]) -> tuple[int, int, int, int]:
        return tuple(self._s(value) for value in box)  # type: ignore[return-value]

    def _s(self, value: float) -> int:
        return int(round(value * self.scale))


def main() -> None:
    parser = argparse.ArgumentParser(description="Render technical explainer PNG/JPG images from a JSON spec.")
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--spec", required=True, help="JSON string or path to a JSON spec file")
    parser.add_argument("--format", default="png", choices=["png", "jpg", "jpeg"])
    parser.add_argument("--size", default="1920x1080")
    args = parser.parse_args()

    width, height = parse_size(args.size)
    spec = load_spec(args.spec)
    renderer = ExplainerRenderer(width, height, args.format)
    paths = renderer.render(spec, args.output_dir)
    for path in paths:
        print(path)


if __name__ == "__main__":
    main()
