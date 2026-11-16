"""Draws the extras' pixel sprites (People Playground style: 35 px per unit, flat 2-3 tone shading, no outlines).

python extras/art/gen_art.py   -> writes the PNGs next to this script. Stdlib only.
"""
import os, struct, zlib, math

HERE = os.path.dirname(os.path.abspath(__file__))


def png(name, w, h, px):
    """px: list of rows, each a list of (r, g, b, a)."""
    raw = b"".join(b"\0" + bytes(c for p in row for c in p) for row in px)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(data)


def hexc(s, a=255):
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), a)


CLEAR = (0, 0, 0, 0)

# Portal gun (the Aperture handheld portal device), barrel to the right, 32 x 14 px:
# W/w/s the white shell (light, mid, shadow), K/k black parts, g the grey underside block, G/H/c the glass core tube
# (mid, highlight, dark) and L the small light on top. The core and the light are drawn again by the game in the colour
# of the last portal fired (`gun_core`, an additive glow over them).
GUN = [
    "......kLLk......................",
    "....sWWWWWWWw...................",
    "..sWWWWWWWWWWWw..........KK.....",
    ".wWWWWWWWWWWWWWWw......KKkKK..KK",
    "wWWWWWWWWWWWWWWWWKK...KK....KK.K",
    "wwwwwwwwwwwwwwwwwKHHHHHKK......K",
    "wwwwwwwwwwwwwwwwwKGGGGGGKKggg...",
    "swwwwwwwwwwwwwwwwKccccccKggggWk.",
    ".swwwwwwwwwwwwwwsKKKKKKKKgkgggk.",
    "..sssswwwwwwwwsssK.....KgggggK.K",
    "....sssssssssssKK.......KKKKK.K.",
    ".........KKk....................",
    "........KKk.....................",
    ".......KKk......................",
]
BODY = {
    "W": hexc("f4f6f7"), "w": hexc("d9dde0"), "s": hexc("aab1b7"), "K": hexc("34393e"), "k": hexc("1c1f22"),
    "g": hexc("8c9399"), "G": hexc("7bb4ad"), "H": hexc("c4ebe5"), "c": hexc("4c7f7a"), "L": hexc("9fd8d0"),
}
# The glow over the core and the light: how much of the portal colour each gets.
CORE = {"G": 200, "H": 255, "c": 120, "L": 255}
TIPS = {"blue": hexc("2f8dff"), "orange": hexc("ff7a12")}


def gun(tint=None):
    rows = []
    for row in GUN:
        out = []
        for ch in row.ljust(len(GUN[0]), "."):
            c = BODY.get(ch, CLEAR)
            if tint and ch in CORE:
                k = CORE[ch] / 255
                c = tuple(int(c[i] * (1 - k * 0.8) + tint[i] * k * 0.8 + 255 * k * 0.2 * (ch in "HL")) for i in range(3)) + (255,)
                c = tuple(min(255, v) for v in c[:3]) + (255,)
            out.append(c)
        rows.append(out)
    return rows


def gun_core():
    """White where the core and light glow (alpha = strength), for the additive glow the game tints."""
    return [[(255, 255, 255, CORE[ch]) if ch in CORE else CLEAR for ch in row.ljust(len(GUN[0]), ".")] for row in GUN]


# A portal: a flat glowing line on the surface, smooth (drawn at 4x the game's density): a white-hot centre fading out
# through the portal's colour across its width, its ends tapering off. LINE_W x LINE_H px spans the opening
# (2 x HALF_LEN) by LINE_H / 4 game pixels.
PORTALS = {
    "blue": (hexc("1a5fd0"), hexc("2f8dff"), hexc("d8f2ff")),
    "orange": (hexc("c24a00"), hexc("ff7a12"), hexc("fff0c8")),
}
LINE_W, LINE_H = 336, 20


def line(colour):
    import math
    dark, mid, light = PORTALS[colour]
    rows = []
    for y in range(LINE_H):
        out = []
        for x in range(LINE_W):
            v = abs(y + 0.5 - LINE_H / 2)
            # The ends: full strength until the last 8% of the length, then a smooth fade (and the line narrows).
            e = min(x + 0.5, LINE_W - x - 0.5) / (LINE_W * 0.08)
            end = 1.0 if e >= 1 else e * e * (3 - 2 * e)
            width = 0.55 + 0.45 * end
            k = math.exp(-(v / (3.4 * width)) ** 2) * end
            hot = math.exp(-(v / (1.4 * width)) ** 2) * end
            # Premultiplied: the game's additive glow adds the colour whatever the alpha.
            c = tuple(int((mid[i] * (1 - hot) + light[i] * hot) * k) for i in range(3))
            out.append(c + (int(255 * k),))
        rows.append(out)
    return rows


if __name__ == "__main__":
    assert all(len(r) <= len(GUN[0]) for r in GUN), "gun rows too wide"
    png("portal_gun.png", len(GUN[0]), len(GUN), gun())
    png("portal_gun_core.png", len(GUN[0]), len(GUN), gun_core())
    # The catalog icon: the gun with a blue core, framed square with 2 px around it.
    g = gun(TIPS["blue"])
    side = len(GUN[0]) + 4
    top = (side - len(GUN)) // 2
    icon = [[CLEAR] * side for _ in range(side)]
    for y, row in enumerate(g):
        icon[top + y][2:2 + len(row)] = row
    png("portal_gun_icon.png", side, side, icon)
    for c in PORTALS:
        png(f"portal_line_{c}.png", LINE_W, LINE_H, line(c))
    print("ok")
