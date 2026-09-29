import zlib
import struct
from pathlib import Path

def create_png(width: int, height: int, color_bg, color_fg) -> bytes:
    # Build raw RGBA pixels
    raw_pixels = bytearray()
    center_x, center_y = width / 2.0, height / 2.0
    radius = min(width, height) / 2.0 - 1.0

    for y in range(height):
        raw_pixels.append(0)  # filter type 0 (None)
        for x in range(width):
            dx = x - center_x
            dy = y - center_y
            dist = (dx * dx + dy * dy) ** 0.5

            # Circular background
            if dist <= radius:
                # Draw a downward arrow / play icon in center
                norm_x = (x - center_x) / radius
                norm_y = (y - center_y) / radius

                # Arrow shaft & head
                is_arrow = (abs(norm_x) < 0.22 and -0.5 <= norm_y <= 0.2) or \
                           (abs(norm_x) < 0.55 and norm_y >= 0.1 and abs(norm_x) <= (0.55 - (norm_y - 0.1) * 1.2))

                if is_arrow:
                    raw_pixels.extend(color_fg)
                else:
                    raw_pixels.extend(color_bg)
            else:
                raw_pixels.extend((0, 0, 0, 0))  # Transparent

    # PNG chunks
    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xffffffff
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    compressed = zlib.compress(bytes(raw_pixels), 9)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", ihdr)
    png += chunk(b"IDAT", compressed)
    png += chunk(b"IEND", b"")
    return png

if __name__ == "__main__":
    icons_dir = Path(__file__).resolve().parent / "icons"
    icons_dir.mkdir(parents=True, exist_ok=True)

    blue_bg = (37, 99, 235, 255)    # #2563EB
    white_fg = (255, 255, 255, 255) # #FFFFFF

    for size in [16, 48, 128]:
        png_data = create_png(size, size, blue_bg, white_fg)
        out_path = icons_dir / f"icon{size}.png"
        out_path.write_bytes(png_data)
        print(f"Generated {out_path} ({size}x{size})")
