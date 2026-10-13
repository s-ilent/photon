#!/usr/bin/env python3
"""Generate the manual-test fixture set for Photon's Image Information window.

Writes /tmp/photon-info-test/ (the two multi-layer DDS files come from a
separate Rust example so they byte-match the format the crate tests emit).
Verifies each file immediately after writing and prints a report.
"""
import math
import os

import numpy as np
from PIL import Image, ImageDraw
from PIL.TiffImagePlugin import IFDRational

OUT = "/tmp/photon-info-test"
os.makedirs(OUT, exist_ok=True)

# --- 01_plain.png: 800x600 RGB gradient, no metadata -------------------------
img = Image.new("RGB", (800, 600))
d = ImageDraw.Draw(img)
for y in range(600):
    t = y / 599
    d.line([(0, y), (799, y)], fill=(int(40 + 60 * t), int(120 + 100 * t), int(220 - 100 * t)))
d.ellipse([620, 40, 740, 160], outline=(255, 255, 255), width=3)
d.rectangle([40, 40, 260, 200], fill=(180, 40, 40))
img.save(f"{OUT}/01_plain.png")

# --- 02_photo_exif.jpg: 640x480 JPEG, Exif + ICC + XMP + IPTC ----------------
photo = Image.new("RGB", (640, 480))
d = ImageDraw.Draw(photo)
for y in range(480):
    t = y / 479
    if t < 0.62:
        d.line([(0, y), (639, y)], fill=(int(90 + 120 * t), int(150 + 80 * t), 235))
    else:
        d.line([(0, y), (639, y)], fill=(int(50 + 60 * t), int(80 + 40 * t), 40))
d.ellipse([500, 60, 580, 140], fill=(255, 240, 180))
d.polygon([(0, 480), (0, 380), (200, 320), (420, 400), (640, 340), (640, 480)], fill=(60, 90, 50))

exif = Image.Exif()
exif[0x010F] = "Photon Test Camera"        # Make
exif[0x0110] = "Model X"                   # Model
exif[0x0131] = "photon fixture 1.0"        # Software
exif[0x0132] = "2026:10:14 12:00:00"       # DateTime
exif_ifd = exif.get_ifd(0x8769)                     # Exif sub-IFD
exif_ifd[0x9003] = "2026:10:13 09:30:00"            # DateTimeOriginal
exif_ifd[0x829A] = IFDRational(1, 250)              # ExposureTime -> "1/250"
exif_ifd[0x829D] = IFDRational(28, 10)              # FNumber -> "2.8"
exif_ifd[0x8827] = 200                              # ISOSpeedRatings
exif_ifd[0x920A] = IFDRational(50)                  # FocalLength -> 50 mm
gps = exif.get_ifd(0x8825)                          # GPS sub-IFD
gps[1] = "S"
gps[2] = (IFDRational(33), IFDRational(51), IFDRational(54))
gps[3] = "E"
gps[4] = (IFDRational(151), IFDRational(12), IFDRational(30))

icc = bytes(range(128))
xmp = (
    '<?xpacket begin="\ufeff" id="W5M0MpCehiHzreSzNTczkc9d"?>'
    '<x:xmpmeta xmlns:x="adobe:ns:meta/"></x:xmpmeta>'
    '<?xpacket end="w"?>'
).encode("utf-8")
iptc = {120: "Photon test fixture", 25: "photon,test,fixture"}

photo.save(
    f"{OUT}/02_photo_exif.jpg",
    quality=90,
    exif=exif,
    icc_profile=icc,
    xmp=xmp,
    iptc=iptc,
)

# --- 03_palette.png: 256x256 indexed (palette) PNG ---------------------------
pal = Image.new("P", (256, 256))
pal.putpalette([c for i in range(256) for c in (i, 255 - i, (i * 7) % 256)])
ppx = pal.load()
for y in range(256):
    for x in range(256):
        ppx[x, y] = (x + y) % 256
pal.save(f"{OUT}/03_palette.png")

# --- 04_gray16.png: 320x240 16-bit grayscale PNG -----------------------------
g16 = Image.new("I;16", (320, 240))
gpx = g16.load()
for y in range(240):
    for x in range(320):
        gpx[x, y] = (x * 65535) // 319
g16.save(f"{OUT}/04_gray16.png")


# --- 05_hdr.hdr: 64x64 Radiance HDR, new-style RLE, values above 1.0 ---------
def to_rgbe(r, g, b):
    m = max(r, g, b)
    if m < 1e-32:
        return (0, 0, 0, 0)
    _, e = math.frexp(m)  # m = f * 2**e with 0.5 <= f < 1
    scale = 2.0 ** (8 - e)
    return (
        min(255, int(r * scale)),
        min(255, int(g * scale)),
        min(255, int(b * scale)),
        e + 128,
    )


W = H = 64
rows_px = []
for y in range(H):
    row = []
    for x in range(W):
        dx, dy = x - 47.0, y - 16.0
        sun = 9.0 * math.exp(-(dx * dx + dy * dy) / 18.0)  # > 1.0: exercises tonemap
        sky = 0.35 + 0.9 * (1.0 - y / H)
        r, g, b = sky * 0.55 + sun, sky * 0.75 + sun, 1.05 + sun
        if y > 46:
            r, g, b = 0.08 + sun * 0.4, 0.16 + sun * 0.5, 0.06 + sun * 0.3
        row.append(to_rgbe(r, g, b))
    rows_px.append(row)

with open(f"{OUT}/05_hdr.hdr", "wb") as f:
    f.write(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n")
    f.write(f"-Y {H} +X {W}\n".encode())
    for row in rows_px:
        f.write(bytes([2, 2, 0, W]))  # new-RLE scanline header
        for ch in range(4):
            vals = bytes(p[ch] for p in row)
            i = 0
            while i < len(vals):
                n = min(128, len(vals) - i)
                f.write(bytes([n]))  # literal run of n (a byte > 128 means a repeat run)
                f.write(vals[i : i + n])
                i += n

# --- 09_big.png: 2048x2048 noisy RGB PNG (multi-MB 'Size' row) ---------------
rng = np.random.default_rng(7)
noise = rng.integers(0, 256, (2048, 2048, 3), dtype=np.uint8)
Image.fromarray(noise, "RGB").save(f"{OUT}/09_big.png")

# --- verify ---------------------------------------------------------------
print(f"fixtures in {OUT}:")
for name in sorted(os.listdir(OUT)):
    path = os.path.join(OUT, name)
    size = os.path.getsize(path)
    with open(path, "rb") as f:
        head = f.read(32)
    note = ""
    if name.endswith(".png"):
        bit_depth, colour_type = head[24], head[25]
        note = f"IHDR bit_depth={bit_depth} colour_type={colour_type}"
    elif name.endswith(".jpg"):
        # marker scan for metadata blocks
        blob = open(path, "rb").read()
        note = (
            f"exif={'Exif\\x00\\x00' in str(blob[:4096]) or blob.find(b'Exif\\x00\\x00') >= 0} "
            f"icc_app2={blob.count(b'ICC_PROFILE')} "
            f"xmp_app1={blob.count(b'ns.adobe.com/xap/1.0')} "
            f"iptc_app13={blob.count(b'Photoshop 3.0')}"
        )
        im = Image.open(path)
        ex = im.getexif()
        note += f" PIL_ifd0={len(ex)} PIL_exif_ifd={len(ex.get_ifd(0x8769))} PIL_gps={len(ex.get_ifd(0x8825))}"
    elif name.endswith(".hdr"):
        note = repr(head[:24])
    print(f"  {name:20s} {size:>10,} bytes  {note}")
