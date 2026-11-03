#!/usr/bin/env python3
"""Numbers for a UX review, with no dependencies.

  measure.py dump FILE...        cut labels and the port hit area in --dump-ui JSON
  measure.py size FILE           WxH that holds every tile in --dump-ui JSON at zoom 1
  measure.py contrast FG BG ...  WCAG contrast of each pair of #RRGGBB colours
  measure.py pixel FILE X Y      the colour of one pixel in an 8-bit RGB or RGBA PNG
"""

import json
import struct
import sys
import zlib

# WCAG 2.5.8 asks for 24 by 24 CSS pixels. One world unit is one pixel at zoom 1.
MIN_TARGET = 24.0
# Text passes at 4.5:1 (WCAG 1.4.3); large text and graphics at 3:1 (1.4.11).
TEXT_RATIO = 4.5
GRAPHIC_RATIO = 3.0
# Space around a whole-patch image, in pixels at zoom 1.
IMAGE_MARGIN = 50


def dump(paths):
    """Print cut labels and the port hit area for each --dump-ui file."""
    for path in paths:
        with open(path) as f:
            ui = json.load(f)
        print(f"== {path}")
        print_cut_labels(ui)
        print_port_size(ui)


def image_size(path):
    """Return the WxH that holds every tile at zoom 1, with a margin on each side."""
    with open(path) as f:
        bounds = [tile["bounds"] for tile in json.load(f)["tiles"]]
    width = max(b["x"] + b["width"] for b in bounds) - min(b["x"] for b in bounds)
    height = max(b["y"] + b["height"] for b in bounds) - min(b["y"] for b in bounds)
    return f"{width + 2 * IMAGE_MARGIN:.0f}x{height + 2 * IMAGE_MARGIN:.0f}"


def print_cut_labels(ui):
    """List labels the tile cuts with an ellipsis (the `shown` field)."""
    cut = [
        f"  {tile['title']}: {item['label']} shows as {item['shown']}"
        for tile in ui["tiles"]
        for part in ("inputs", "outputs", "widgets")
        for item in tile.get(part, [])
        if "shown" in item
    ]
    print(f"Cut labels: {len(cut)}")
    print("\n".join(cut) if cut else "  none")


def print_port_size(ui):
    """Print the port hit area. Widget `bounds` are grid cells, not targets, so they are not measured."""
    port = ui["port_hit_radius"] * 2
    verdict = "under" if port < MIN_TARGET else "meets"
    print(f"Port hit area: {port:.0f} px across at zoom 1, {verdict} {MIN_TARGET:.0f} px")


def luminance(colour):
    """Return the WCAG relative luminance of a #RRGGBB colour."""
    value = colour.lstrip("#")
    if len(value) != 6:
        raise ValueError(f"expected #RRGGBB, got {colour}")
    channels = [int(value[i : i + 2], 16) / 255 for i in (0, 2, 4)]
    r, g, b = [c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in channels]
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast_ratio(a, b):
    """Return the WCAG contrast ratio of two colours, from 1 to 21."""
    high, low = sorted((luminance(a), luminance(b)), reverse=True)
    return (high + 0.05) / (low + 0.05)


def contrast(colours):
    """Print the ratio of each foreground and background pair, and what it passes."""
    if not colours or len(colours) % 2:
        raise SystemExit("contrast takes pairs of colours: FG BG [FG BG ...]")
    for fg, bg in zip(colours[::2], colours[1::2]):
        ratio = contrast_ratio(fg, bg)
        verdict = "meets 4.5:1 and 3:1" if ratio >= TEXT_RATIO else "meets 3:1 only, enough for large text and graphics" if ratio >= GRAPHIC_RATIO else "under 3:1"
        print(f"{fg} on {bg}: {ratio:.2f}:1, {verdict}")


def pixel(path, x, y):
    """Return the #RRGGBB colour at physical pixel (x, y) of a non-interlaced 8-bit PNG."""
    with open(path, "rb") as f:
        data = f.read()
    width, height, depth, colour_type, _, _, interlace = struct.unpack(">IIBBBBB", data[16:29])
    channels = {2: 3, 6: 4}.get(colour_type)
    if depth != 8 or channels is None or interlace:
        raise SystemExit(f"{path}: only non-interlaced 8-bit RGB or RGBA PNG files are read")
    if not (0 <= x < width and 0 <= y < height):
        raise SystemExit(f"{path}: ({x}, {y}) is outside the image, {width} by {height}")
    chunks, at = [], 8
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at : at + 8])
        if kind == b"IDAT":
            chunks.append(data[at + 8 : at + 8 + length])
        at += 12 + length
    raw = zlib.decompress(b"".join(chunks))
    stride = width * channels
    row = bytearray(stride)
    # Rows must be decoded from the top, because each filter reads the row above.
    for line in range(y + 1):
        start = line * (stride + 1)
        kind, above, row = raw[start], row, bytearray(raw[start + 1 : start + 1 + stride])
        for i in range(stride):
            left = row[i - channels] if i >= channels else 0
            up = above[i]
            corner = above[i - channels] if i >= channels else 0
            row[i] = (row[i] + unfilter(kind, left, up, corner)) & 255
    return "#{:02X}{:02X}{:02X}".format(*row[x * channels : x * channels + 3])


def unfilter(kind, left, up, corner):
    """Return what a PNG filter of this kind subtracted from a byte."""
    if kind == 0:
        return 0
    if kind == 1:
        return left
    if kind == 2:
        return up
    if kind == 3:
        return (left + up) // 2
    estimate = left + up - corner
    distances = (abs(estimate - left), abs(estimate - up), abs(estimate - corner))
    return (left, up, corner)[distances.index(min(distances))]


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "dump" and len(sys.argv) > 2:
        dump(sys.argv[2:])
    elif command == "size" and len(sys.argv) == 3:
        print(image_size(sys.argv[2]))
    elif command == "contrast":
        contrast(sys.argv[2:])
    elif command == "pixel" and len(sys.argv) == 5:
        print(pixel(sys.argv[2], int(sys.argv[3]), int(sys.argv[4])))
    else:
        raise SystemExit(__doc__)
