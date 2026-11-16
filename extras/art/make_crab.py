"""Draws the crab's sprite sheets (skin, flesh, bone) and its catalog icon, and writes ../src/sheet.rs with each part's
rect. Flat pixel art at the Gorse's density (35 pixels per unit), 2-3 tones, no outlines, no noise. Ferris the crab
(rustacean.net, public domain) is the model: a wide dome with a zigzag top, a darker underside band, big black eyes with
white highlights and a small smile on the front, round pincer claws and thin bent legs. Run: python make_crab.py"""
import os
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
W, H = 48, 32
BASE, RIM, HI = (247, 76, 0), (222, 64, 0), (255, 104, 30)
UNDER, UNDER_RIM = (176, 48, 6), (150, 38, 4)
EYE, SHINE, MOUTH = (16, 12, 12), (255, 255, 255), (40, 10, 4)
FLESH, FLESH_RIM = (238, 206, 164), (216, 176, 132)
BONE = (242, 238, 224)

# name: (x, y from top, w, h). Every part but the shell hangs from its joint at the top edge and points down.
PARTS = {
    "shell": (0, 0, 28, 16),
    "upper leg": (29, 0, 3, 8),
    "lower leg": (33, 0, 3, 12),
    "arm": (37, 0, 3, 8),
    "claw": (0, 17, 9, 10),
    "claw mirrored": (10, 17, 9, 10),
    "pincer": (20, 17, 4, 6),
}

skin = Image.new("RGBA", (W, H))
flesh = Image.new("RGBA", (W, H))
bone = Image.new("RGBA", (W, H))


def put(img, part, x, y, c):
    px, py, w, h = PARTS[part]
    assert 0 <= x < w and 0 <= y < h, (part, x, y)
    img.putpixel((px + x, py + y), c + (255,))


def edge(cells, c):
    return any((c[0] + dx, c[1] + dy) not in cells for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))


def shade(cells, base=BASE, rim=RIM):
    """Flat fill with a slightly darker 1 px rim."""
    return {c: rim if edge(cells, c) else base for c in cells}


def fill(part, colours):
    """Skin as given; the flesh fills the same silhouette with its own rim."""
    for (x, y), c in colours.items():
        put(skin, part, x, y, c)
    for (x, y), c in shade(set(colours), FLESH, FLESH_RIM).items():
        put(flesh, part, x, y, c)


def bones(part, cells):
    for x, y in cells:
        put(bone, part, x, y, BONE)


def art(rows, y0=0):
    """Cells of an ASCII drawing: '#' filled."""
    return {(x, y0 + y) for y, r in enumerate(rows) for x, ch in enumerate(r) if ch == "#"}


# Shell: the dome (rows 2-15) with a zigzag of spikes around its top, the underside band below the face.
shell = art([
    "........#...#..#...#........",
    ".......###.######.###.......",
    "......################......",
    "..#.####################.#..",
    "...######################...",
    "..########################..",
    ".##########################.",
    "############################",
    ".##########################.",
    "############################",
    "############################",
    "############################",
    ".##########################.",
    "..########################..",
    "....####################....",
    "........############........",
])
colours = shade(shell)
for (x, y) in shell:
    if 4 <= y <= 6 and 8 <= x <= 19 and not edge(shell, (x, y)):
        colours[(x, y)] = HI  # the dome's light top
    if y >= 13:
        colours[(x, y)] = UNDER_RIM if edge(shell, (x, y)) else UNDER
for ex in (8, 17):  # eyes: 3x4 black ovals with a white shine
    for x in range(ex, ex + 3):
        for y in range(8, 12):
            colours[(x, y)] = EYE
    colours[(ex + 1, 9)] = SHINE
for x, y in [(12, 12), (13, 12), (14, 12), (15, 12), (13, 13), (14, 13)]:
    colours[(x, y)] = MOUTH
fill("shell", colours)
bones("shell", [(x, 7) for x in range(4, 24)] + [(x, y) for x in (13, 14) for y in range(3, 14)])


def segment(part, w, h, taper):
    cells = {(x, y) for x in range(w) for y in range(h)}
    if taper:  # a pointed foot
        cells -= {(0, h - 1), (w - 1, h - 1), (0, h - 2)}
    fill(part, shade(cells))
    bones(part, [(1, y) for y in range(1, h - 1)])


segment("upper leg", 3, 8, False)
segment("lower leg", 3, 12, True)
segment("arm", 3, 8, False)

# Claw: a round palm and the outer (fixed) finger curling in; the pincer limb is the inner finger. Together they make
# Ferris' round claw with its notch.
CLAW = [
    "..#####..",
    ".#######.",
    "#########",
    "#########",
    "#########",
    ".....####",
    "......###",
    "......###",
    ".....###.",
    "....##...",
]


def claw(part, mirror):
    cells = art(CLAW)
    colours = shade(cells)
    for c in [(3, 1), (4, 1), (5, 1), (2, 2), (3, 2)]:
        colours[c] = HI
    if mirror:
        colours = {(8 - x, y): c for (x, y), c in colours.items()}
    fill(part, colours)
    bones(part, [(4, y) for y in range(1, 5)] + [(1 if mirror else 7, y) for y in range(5, 8)])


claw("claw", False)
claw("claw mirrored", True)

# Pincer: the moving finger, hinged at its top, symmetric so it serves either claw.
fill("pincer", shade(art(["####", "####", "####", "####", ".##.", ".##."])))
bones("pincer", [(1, y) for y in range(1, 4)] + [(2, y) for y in range(1, 4)])

for name, img in [("crab-skin", skin), ("crab-flesh", flesh), ("crab-bone", bone)]:
    img.save(os.path.join(HERE, name + ".png"))

# Catalog icon: Ferris front on, claws raised, built from the parts.
icon = Image.new("RGBA", (48, 34))


def stamp(part, ox, oy, flip_v=False, angle=0):
    px, py, w, h = PARTS[part]
    p = skin.crop((px, py, px + w, py + h))
    if flip_v:
        p = p.transpose(Image.FLIP_TOP_BOTTOM)
    if angle:
        p = p.rotate(angle, expand=True, resample=Image.NEAREST)
    icon.alpha_composite(p, (ox, oy))


for s in (-1, 1):
    for i in (0, 2):  # two legs a side: thigh out sideways from under the shell, shin down to the ground
        y = 18 + i
        knee = 24 + s * (12 + 4 * i)
        stamp("upper leg", min(knee, 24 + s * (6 + 4 * i)), y, angle=90)
        stamp("lower leg", knee - 1, y)
    stamp("arm", 24 + s * 14 - 1, 9, flip_v=True)
stamp("shell", 10, 8)
stamp("claw", 2, 1, flip_v=True)
stamp("pincer", 3, 0, flip_v=True)
stamp("claw mirrored", 37, 1, flip_v=True)
stamp("pincer", 41, 0, flip_v=True)
icon.save(os.path.join(HERE, "crab-icon.png"))

with open(os.path.join(HERE, "..", "src", "sheet.rs"), "w", newline="\n") as f:
    f.write("// Written by art/make_crab.py: each part's rect in the crab sheets, Unity style (x, y from the bottom, w, h).\n")
    f.write(f"pub const SHEET: [u32; 2] = [{W}, {H}];\n")
    f.write(f"pub const ICON: [u32; 2] = [{icon.width}, {icon.height}];\n")
    f.write("pub const RECTS: &[(&str, [u32; 4])] = &[\n")
    for name, (x, y, w, h) in PARTS.items():
        f.write(f"    ({name!r:}, [{x}, {H - y - h}, {w}, {h}]),\n".replace("'", '"'))
    f.write("];\n")
