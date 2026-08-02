#!/usr/bin/env python3
"""Check the free-surface snapshot against the claims made about it.

This reads pixels, not intentions. Every number it prints comes out of the PNG
the host actually wrote, so a regression in the panel pass shows up here rather
than in prose nobody re-ran.

It answers exactly three questions and refuses to answer any others:

1. Does the panel backdrop blend with what is under it, or replace it?
2. Does the panel edge land where a fractional coordinate puts it, off the
   cell raster the grid is confined to?
3. Does grid text under the panel survive as a distinguishable colour?

It does not decide whether a free surface is the right home for navigation.
That is `open_question neovim_glsl.navigation_surface_decision`, and this file
only supplies evidence for it.
"""
import json
import pathlib
import struct
import sys
import zlib
from collections import Counter

HERE = pathlib.Path(__file__).resolve().parent
PNG = HERE / "out" / "free-surface-over-grid.png"
PANELS = HERE / "panels.json"
MEASUREMENT = HERE / "out" / "measurement.json"

# Panel coordinates live in the renderer's own pixel space, which is the
# framebuffer's: the shader is handed the physical size, so nothing is rescaled
# between the JSON and the PNG. Stating it stops this check from silently
# comparing logical pixels against device pixels.
SCALE = 1


def read_png(path):
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path} is not a PNG")
    i, idat, w, h, ct = 8, b"", None, None, None
    while i < len(data):
        ln = struct.unpack(">I", data[i : i + 4])[0]
        typ = data[i + 4 : i + 8]
        chunk = data[i + 8 : i + 8 + ln]
        i += 12 + ln
        if typ == b"IHDR":
            w, h, _, ct, _, _, _ = struct.unpack(">IIBBBBB", chunk)
        elif typ == b"IDAT":
            idat += chunk
        elif typ == b"IEND":
            break
    if ct != 6:
        raise SystemExit(f"expected RGBA, got colour type {ct}")
    raw = zlib.decompress(idat)
    bpp, stride = 4, w * 4
    out, prev, p = bytearray(), bytearray(stride), 0
    for _ in range(h):
        f = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        if f == 1:
            for x in range(bpp, stride):
                line[x] = (line[x] + line[x - bpp]) & 255
        elif f == 2:
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 255
        elif f == 3:
            for x in range(stride):
                a = line[x - bpp] if x >= bpp else 0
                line[x] = (line[x] + ((a + prev[x]) >> 1)) & 255
        elif f == 4:
            for x in range(stride):
                a = line[x - bpp] if x >= bpp else 0
                c = prev[x - bpp] if x >= bpp else 0
                b = prev[x]
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        elif f != 0:
            raise SystemExit(f"unknown PNG filter {f}")
        out += line
        prev = line
    return w, h, bytes(out)


def blend(top, bottom, alpha):
    return tuple(round(alpha * t + (1 - alpha) * b) for t, b in zip(top, bottom))


def unpack(hexstr):
    v = int(hexstr.lstrip("#"), 16)
    return ((v >> 16) & 255, (v >> 8) & 255, v & 255)


def near(a, b, tol=2):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def main():
    w, h, px = read_png(PNG)
    panels = json.loads(PANELS.read_text())
    stats = json.loads(MEASUREMENT.read_text())["stats"]
    failures = []

    def at(x, y):
        o = (y * w + x) * 4
        return tuple(px[o : o + 3])

    print(f"snapshot {PNG.name}: {w}x{h}")

    def inside_any_panel(x, y):
        for p in panels:
            px0, py0 = p["x"] * SCALE, p["y"] * SCALE
            if px0 <= x <= px0 + p["w"] * SCALE and py0 <= y <= py0 + p["h"] * SCALE:
                return True
        return False

    # The grid own background: the most common colour anywhere outside every
    # panel. Sampling one fixed point would land on a glyph sooner or later and
    # make every later comparison quietly wrong.
    outside = Counter(
        at(x, y)
        for y in range(0, h, 3)
        for x in range(0, w, 3)
        if not inside_any_panel(x, y)
    )
    grid_bg = outside.most_common(1)[0][0]
    print(f"grid background outside every panel: {grid_bg}")

    for idx, (p, st) in enumerate(zip(panels, stats)):
        x0, y0 = p["x"] * SCALE, p["y"] * SCALE
        x1, y1 = x0 + p["w"] * SCALE, y0 + p["h"] * SCALE
        alpha, bg = p["alpha"], unpack(p["bg"])
        print(f"\npanel {idx}: origin ({p['x']}, {p['y']}) size {p['w']}x{p['h']} alpha {alpha}")

        # 1. Blending. The dominant colour inside the panel is its backdrop over
        #    whatever the grid was showing there. If the panel replaced the grid
        #    instead of blending, that colour would be `bg` exactly.
        region = Counter()
        for y in range(int(y0) + 8, int(y1) - 8, 2):
            for x in range(int(x0) + 8, int(x1) - 8, 2):
                region[at(x, y)] += 1
        dominant, count = region.most_common(1)[0]
        want = blend(bg, grid_bg, alpha)
        print(f"  dominant interior colour: {dominant} ({count} samples)")
        print(f"  blend of {bg} at {alpha} over {grid_bg} predicts {want}; opaque would be {bg}")
        if not near(dominant, want, 3):
            failures.append(f"panel {idx}: interior {dominant} is not the blend {want}")
        if near(dominant, bg, 1) and alpha < 0.99:
            failures.append(f"panel {idx}: interior is opaque despite alpha {alpha}")

        # 2. Off-grid origin: the reported top edge must land where a fractional
        #    coordinate puts it, which is not where a cell boundary is.
        if not st["origin_off_grid"]:
            failures.append(f"panel {idx}: host did not report the origin as off-grid")
        col = int(x0 + 60)
        edge = next(
            (y for y in range(int(y0) - 14, int(y0) + 14) if at(col, y) != grid_bg),
            None,
        )
        print(f"  top edge in the image: row {edge}, coordinate asked for {y0}")
        if edge is None or abs(edge - y0) > 2:
            failures.append(f"panel {idx}: top edge at {edge}, expected near {y0}")

        # 3. What the host said it emitted, echoed so the two files stay married.
        print(
            f"  host reported: {st['quads']} quads, {st['clipped_quads']} clipped, "
            f"{st['rows_visible']} rows visible, first row cut {st['first_row_clip_px']}px, "
            f"last row cut {st['last_row_clip_px']}px"
        )
        if st["first_row_clip_px"] != p.get("scroll", 0.0) % p["row_height"]:
            failures.append(
                f"panel {idx}: first-row clip {st['first_row_clip_px']} does not follow "
                f"from scroll {p.get('scroll', 0.0)} at pitch {p['row_height']}"
            )

    # 4. Grid text under the first panel is still there, dimmed by the backdrop
    #    rather than erased by it.
    p0 = panels[0]
    x0, y0 = p0["x"] * SCALE, p0["y"] * SCALE
    x1, y1 = x0 + p0["w"] * SCALE, y0 + p0["h"] * SCALE
    grid_fg = next(
        (
            at(x, y)
            for y in range(h - 220, h - 30)
            for x in range(30, 700)
            if sum(at(x, y)) > 600
        ),
        None,
    )
    if grid_fg is None:
        raise SystemExit("no grid text found outside the panels; the bed script did not run")
    want_through = blend(unpack(p0["bg"]), grid_fg, p0["alpha"])
    found = sum(
        1
        for y in range(int(y0) + 20, int(y1) - 20, 2)
        for x in range(int(x1) - 300, int(x1) - 20, 2)
        if near(at(x, y), want_through, 4)
    )
    print(f"\ngrid text outside the panel: {grid_fg}")
    print(f"through panel 0 it should read {want_through}; matching pixels inside: {found}")
    if found < 50:
        failures.append("grid text under the panel is not visible through it")

    print()
    if failures:
        for f in failures:
            print(f"FAIL {f}")
        return 1
    print("ok: backdrops blend, origins sit off the cell raster, grid text shows through")
    return 0


if __name__ == "__main__":
    sys.exit(main())
