#!/usr/bin/env python3
"""Genera el icono de NovaCut sin dependencias externas.

Diseño: cuadrado redondeado oscuro con degradado, triángulo de reproducción
con esquinas suaves y un corte diagonal de "tijera" que lo atraviesa.
Salidas: assets/icon.png (512), assets/icon.ico (multi-tamaño) y
assets/icon_64.rgba (RGBA crudo para el icono de ventana en eframe).
"""

import math
import struct
import zlib
from pathlib import Path

SIZE = 512
SS = 2  # supermuestreo

BG_TOP = (14, 26, 36)
BG_BOTTOM = (6, 12, 18)
GLOW = (0, 168, 181)
TRI_TOP = (53, 208, 197)
TRI_BOTTOM = (14, 148, 163)
BORDER = (30, 58, 71)
BLADE = (235, 250, 252)


def png_write(path: Path, size: int, rows: list[bytes]) -> None:
    raw = b"".join(b"\x00" + row for row in rows)

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return (
            struct.pack(">I", len(data))
            + body
            + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    data = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    path.write_bytes(data)


def clamp(value: float, low: float, high: float) -> float:
    return max(low, min(high, value))


def sd_rounded_box(px: float, py: float, half: float, radius: float) -> float:
    qx = abs(px) - half + radius
    qy = abs(py) - half + radius
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius


def sd_segment(px: float, py: float, ax: float, ay: float, bx: float, by: float) -> float:
    pax, pay = px - ax, py - ay
    bax, bay = bx - ax, by - ay
    h = clamp((pax * bax + pay * bay) / (bax * bax + bay * bay), 0.0, 1.0)
    dx, dy = pax - bax * h, pay - bay * h
    return math.hypot(dx, dy)


def sd_tri_iso(p: tuple, q: tuple) -> float:
    """SDF exacto de triángulo isósceles (IQ): apex en el origen, base en y=q[1]."""
    px, py = abs(p[0]), p[1]
    qx, qy = q
    t = clamp((px * qx + py * qy) / (qx * qx + qy * qy), 0.0, 1.0)
    ax, ay = px - qx * t, py - qy * t
    tx = clamp(px / qx, 0.0, 1.0)
    bx, by = px - qx * tx, py - qy
    s = -math.copysign(1.0, qy)
    # GLSL min() es componente a componente: la pareja (dist², borde) no se
    # elige junta, cada componente se minimiza por separado (de eso depende
    # el signo correcto del SDF).
    dx = min(ax * ax + ay * ay, bx * bx + by * by)
    dy = min(s * (px * qy - py * qx), s * (py - qy))
    return -math.sqrt(max(dx, 0.0)) * math.copysign(1.0, dy)


def sd_triangle_play(px: float, py: float, half_w: float, height: float, radius: float) -> float:
    """Play apuntando a la derecha: apex en (+height/2, 0), base en -height/2."""
    # Espacio del triángulo: apex en el origen, base en v=-height.
    u = py
    v = px - height / 2.0
    return sd_tri_iso((u, v), (half_w, -height)) - radius


def lerp(a: tuple, b: tuple, t: float) -> tuple:
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))


def render() -> list[bytes]:
    scale = SIZE * SS
    cx = cy = scale / 2.0
    half = scale / 2.0 - 6 * SS
    radius = 96 * SS
    tri_cx, tri_cy = cx + 8 * SS, cy + 4 * SS
    tri_w, tri_h, tri_r = 100 * SS, 170 * SS, 22 * SS
    blade_ax, blade_ay = cx - 118 * SS, cy - 190 * SS
    blade_bx, blade_by = cx - 44 * SS, cy + 196 * SS
    rows = []
    for y in range(scale):
        row = bytearray()
        for x in range(scale):
            px, py = x + 0.5 - cx, y + 0.5 - cy
            # Fondo con degradado vertical y brillo cian abajo-izquierda.
            t = clamp((y / scale) * 1.1, 0.0, 1.0)
            color = lerp(BG_TOP, BG_BOTTOM, t)
            glow_d = math.hypot(px + 130 * SS, py - 150 * SS) / (scale * 0.9)
            glow = clamp(1.0 - glow_d, 0.0, 1.0) ** 2 * 0.35
            color = lerp(color, GLOW, glow)
            d_box = sd_rounded_box(px, py, half, radius)
            # px/py ya son relativos al centro; el triángulo solo se desvía
            # unos píxeles de ese centro.
            d_tri = sd_triangle_play(
                px - (tri_cx - cx), py - (tri_cy - cy), tri_w, tri_h, tri_r
            )
            # Corte: hueco del color de fondo + línea de brillo a la izquierda.
            d_blade = sd_segment(
                x + 0.5, y + 0.5, blade_ax, blade_ay, blade_bx, blade_by
            )
            gap = 5.0 * SS
            if d_blade <= gap:
                if d_tri < 0:
                    color = lerp(BG_TOP, BG_BOTTOM, t)
            fill_alpha = clamp(0.5 - d_box / SS, 0.0, 1.0)
            tri_alpha = clamp(0.5 - d_tri / SS, 0.0, 1.0)
            blade_alpha = clamp((gap - 1.6 * SS - d_blade) / SS, 0.0, 1.0)
            grad = clamp((py + tri_h) / (2 * tri_h), 0.0, 1.0)
            tri_color = lerp(TRI_TOP, TRI_BOTTOM, grad)
            if tri_alpha > 0:
                if d_blade > gap:
                    color = lerp(color, tri_color, tri_alpha)
            if blade_alpha > 0 and d_tri < 2.0 * SS and d_blade < gap + 2.0 * SS:
                color = lerp(color, BLADE, blade_alpha * 0.9)
            border_alpha = clamp(0.5 - abs(d_box + 1.5 * SS) / SS, 0.0, 1.0)
            if border_alpha > 0:
                color = lerp(color, BORDER, border_alpha * 0.8)
            row.extend(int(clamp(c, 0, 255)) for c in color)
            row.append(int(fill_alpha * 255))
        rows.append(bytes(row))
    return rows


def downscale(rows: list[bytes], factor: int) -> list[bytes]:
    size = len(rows) // factor
    out = []
    for oy in range(size):
        row = bytearray()
        for ox in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(factor):
                src = rows[oy * factor + sy]
                for sx in range(factor):
                    i = (ox * factor + sx) * 4
                    acc[0] += src[i]
                    acc[1] += src[i + 1]
                    acc[2] += src[i + 2]
                    acc[3] += src[i + 3]
            n = factor * factor
            row.extend(c // n for c in acc)
        out.append(bytes(row))
    return out


def ico_write(path: Path, entries: list[tuple[int, list[bytes]]]) -> None:
    images = []
    for size, rows in entries:
        png_path = path.with_name(f"ico_tmp_{size}.png")
        png_write(png_path, size, rows)
        images.append((size, png_path.read_bytes()))
        png_path.unlink()
    header = struct.pack("<HHH", 0, 1, len(images))
    directory = b""
    offset = 6 + 16 * len(images)
    for size, blob in images:
        dim = 0 if size >= 256 else size
        directory += struct.pack(
            "<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset
        )
        offset += len(blob)
    path.write_bytes(header + directory + b"".join(blob for _, blob in images))


def main() -> None:
    assets = Path(__file__).resolve().parent.parent / "assets"
    assets.mkdir(exist_ok=True)
    rows = render()
    # Bajamos de la versión supermuestreada a la resolución final de 512.
    final_rows = downscale(rows, SS)
    png_write(assets / "icon.png", SIZE, final_rows)
    entries = [
        (size, downscale(final_rows, SIZE // size))
        for size in (256, 128, 64, 48, 32, 16)
    ]
    ico_write(assets / "icon.ico", entries)
    (assets / "icon_64.rgba").write_bytes(
        b"".join(downscale(final_rows, SIZE // 64))
    )
    print("iconos generados en", assets)


if __name__ == "__main__":
    main()
