#!/usr/bin/env python3
"""Writes crates/verba-app/assets/verba.ico (sizes 16, 24, 32, 48, 64, 256).

The design is the one drawn at run time by make_icon() in crates/verba-app/src/main.rs:
a round Windows-blue (#0078D7) disc with a white "V" made of two thick strokes. Pure Python, no packages.

    python scripts/make_icon.py
"""
import math
import os
import struct
import zlib

SIZES = [16, 24, 32, 48, 64, 256]
BLUE = (0, 120, 215)
WHITE = (255, 255, 255)
SS = 4  # supersampling per axis, for smooth edges


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def coverage(x, y, n):
    """(r, g, b, a) of one pixel, as the average of SS*SS samples. Geometry is drawn on a 32 unit grid."""
    k = 32.0 / n
    r = g = b = a = 0.0
    for sy in range(SS):
        for sx in range(SS):
            px = (x + (sx + 0.5) / SS) * k
            py = (y + (sy + 0.5) / SS) * k
            if math.hypot(px - 16.0, py - 16.0) > 15.5:
                continue
            v = min(seg_dist(px, py, 9.0, 10.0, 16.0, 23.0), seg_dist(px, py, 23.0, 10.0, 16.0, 23.0))
            c = WHITE if v <= 2.4 else BLUE
            r += c[0]
            g += c[1]
            b += c[2]
            a += 1.0
    total = SS * SS
    if a == 0:
        return (0, 0, 0, 0)
    return (round(r / a), round(g / a), round(b / a), round(255 * a / total))


def png(n):
    raw = bytearray()
    for y in range(n):
        raw.append(0)  # filter: none
        for x in range(n):
            raw.extend(coverage(x, y, n))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0))  # 8 bit RGBA
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def main():
    images = [(n, png(n)) for n in SIZES]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for n, data in images:
        entries += struct.pack("<BBBBHHII", 0 if n >= 256 else n, 0 if n >= 256 else n, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "verba-app", "assets", "verba.ico")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "wb") as f:
        f.write(header + entries + b"".join(d for _, d in images))
    print(f"wrote {os.path.normpath(out)} ({offset} bytes, sizes {SIZES})")


if __name__ == "__main__":
    main()
