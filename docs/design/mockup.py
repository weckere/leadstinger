#!/usr/bin/env python3
"""Generate a FigCraft command script for the LeadStinger design mockup.

Palette and shapes follow SoundCraft's dark theme (MIT/Apache, storytold/soundcraft):
clip-style tiles (name bar = colour x0.9, body = colour x0.34, waveform tinted toward
white), green-on-black counter, Pro Tools-style keys and meters. Fonts: Inter only,
with tabular figures for times.
Usage: mockup.py NAME OUT.json TOKENS.json
"""
import json, math, random, sys

NAME = sys.argv[1] if len(sys.argv) > 1 else "LeadStinger"
OUT = sys.argv[2] if len(sys.argv) > 2 else "mockup.json"
TOKENS_OUT = sys.argv[3] if len(sys.argv) > 3 else "tokens.json"

# SoundCraft dark tokens (+ light where it defines them).
DARK = {
    "window": "#1E1E1E", "panel": "#252526", "panel2": "#2C2C2D", "bar": "#161617",
    "group": "#181819", "border": "#0E0E0E", "borderLight": "#404042",
    "text": "#D6D6D6", "textDim": "#969698", "accent": "#4084C4", "accentDark": "#285C92",
    "key": "#3A3A3C", "keyHover": "#4E4E50", "keyBorder": "#141414", "selected": "#344660",
    "counterBg": "#040404", "counterText": "#68DC78", "playhead": "#E83C32",
    "meterGreen": "#3CC850", "meterYellow": "#E6D23C", "meterRed": "#E63228", "meterOff": "#080808",
    "stop": "#6E1E1E", "playing": "#285A32", "solo": "#DEC434",
}
LIGHT = dict(DARK, window="#B8BABE", panel="#CCCED2", panel2="#C2C4C8", bar="#AEB0B4",
             group="#C4C6CA", border="#8E9094", borderLight="#9EA0A4", text="#18191C",
             textDim="#4A4C50", accent="#3E74B4", key="#DADCE0", keyHover="#E6E8EC",
             keyBorder="#8E9094", selected="#A8BCD8", meterOff="#34363A",
             meterGreen="#24A03A", meterYellow="#BEA214", meterRed="#C32A23")
TRACK = ["#5684D6", "#5EAAD6", "#50BEA8", "#68BA62", "#A8C44C", "#DEC448", "#E6983E", "#DE6848",
         "#D6526E", "#C456A8", "#9662CE", "#6E68D6", "#8296AA", "#B28C6E", "#78AA8C", "#BE7896"]
T = DARK
FONT = "Inter"


def hx(c):
    return tuple(int(c[i:i + 2], 16) for i in (1, 3, 5))


def to_hex(rgb):
    return "#" + "".join(f"{max(0, min(255, round(v))):02X}" for v in rgb)


def mul(c, f):
    return to_hex(v * f for v in hx(c))


def tint(c, t):
    return to_hex(v + (255 - v) * t for v in hx(c))


steps = []


def add(command, params):
    steps.append({"command": command, "params": params})
    return {"$step": len(steps) - 1}


def frame(parent, name, x, y, w, h, fill=None, radius=0, stroke=None, clip=True, **props):
    # FigCraft frames default to a white fill; no fill means an explicitly empty list.
    p = {"clip": clip, "fills": [fill] if fill else []}
    if radius:
        p["cornerRadius"] = radius
    if stroke:
        p["stroke"] = stroke
        p["strokeWeight"] = props.pop("strokeWeight", 1)
        p["strokeAlign"] = "inside"
    p.update(props)
    return add("node.create", {"type": "frame", "parent": parent, "name": name, "x": x, "y": y,
                               "width": w, "height": h, "props": p, "select": False})


def rect(parent, name, x, y, w, h, fill, radius=0, **props):
    p = {"fill": fill}
    if radius:
        p["cornerRadius"] = radius
    p.update(props)
    return add("node.create", {"type": "rectangle", "parent": parent, "name": name, "x": x, "y": y,
                               "width": w, "height": h, "props": p, "select": False})


def text(parent, s, x, y, size=13, style="Regular", color=None, w=None, align="left", tnum=False, **props):
    p = {"fontFamily": FONT, "fontStyle": style, "fontSize": size, "fill": color or T["text"],
         "textAlign": align}
    if tnum:
        p["features"] = {"tnum": True}
    if w:
        p["width"] = w
        p["textAutoResize"] = "height"
    p.update(props)
    return add("node.create", {"type": "text", "parent": parent, "text": s, "x": x, "y": y,
                               "props": p, "select": False})


def vector(parent, name, x, y, w, h, path, fill):
    return add("node.create", {"type": "vector", "parent": parent, "name": name, "x": x, "y": y,
                               "width": w, "height": h, "path": path,
                               "props": {"fill": fill}, "select": False})


def key(parent, label, x, y, w=None, h=26, on=False, fill=None, color=None, style="SemiBold"):
    w = w or (len(label) * 7.2 + 22)
    k = frame(parent, f"key {label}", x, y, w, h, fill or (T["accentDark"] if on else T["key"]), 3,
              stroke=T["keyBorder"])
    rect(k, "top light", 1, 1, w - 2, 1, "#FFFFFF1F")
    text(k, label, 0, (h - 15) / 2 - 1, 12, style, color or ("#FFFFFF" if on else T["text"]), w=w, align="center")
    return w


def waveform_path(w, h, seed, upto=None, density=2.0):
    """Mirrored envelope of a pseudo-random but smooth signal, as SVG path data."""
    rnd = random.Random(seed)
    n = int(w / density)
    phases = [rnd.uniform(0, 6.28) for _ in range(4)]
    freqs = [rnd.uniform(0.5, 1.5), rnd.uniform(2, 4), rnd.uniform(6, 11), rnd.uniform(15, 25)]
    shape = rnd.choice(["swell", "hit", "steady", "fade"])
    pts = []
    for i in range(n + 1):
        t = i / n
        if upto is not None and t > upto:
            break
        env = {"swell": math.sin(math.pi * t) ** 0.7, "hit": math.exp(-4 * t) * 0.95 + 0.05,
               "steady": 0.75 + 0.15 * math.sin(6 * t), "fade": 1 - 0.85 * t}[shape]
        wobble = sum(math.sin(f * t * 6.28 + p) * a for f, p, a in zip(freqs, phases, [0.35, 0.25, 0.2, 0.15]))
        amp = max(0.04, min(1.0, env * (0.62 + 0.38 * wobble)))
        pts.append((i * density, amp * h / 2 * 0.92))
    mid = h / 2
    top = " ".join(f"L{x:.1f} {mid - a:.1f}" for x, a in pts)
    bottom = " ".join(f"L{x:.1f} {mid + a:.1f}" for x, a in reversed(pts))
    x0 = pts[0][0]
    return f"M{x0:.1f} {mid:.1f} {top} {bottom} Z", (pts[-1][0] if pts else 0)


# ── Design tokens as FigCraft variables (Dark + Light), exported for the app ────────
add("variable.createCollection", {"name": NAME, "modes": ["Dark", "Light"]})
for k in DARK:
    add("variable.create", {"collection": NAME, "name": k, "type": "color",
                            "values": {"Dark": DARK[k], "Light": LIGHT[k]}})
for i, c in enumerate(TRACK):
    add("variable.create", {"collection": NAME, "name": f"tile/{i + 1:02}", "type": "color", "value": c})
add("variable.export", {"collections": [NAME], "format": "json", "path": TOKENS_OUT})

# ── Window ──────────────────────────────────────────────────────────────────────────
W, H = 1280, 800
win = frame(None, f"{NAME} — main window", 0, 0, W, H, T["window"], 10, stroke=T["border"])

# Title + toolbar band
bar = frame(win, "toolbar", 0, 0, W, 64, T["panel"])
rect(bar, "toolbar bottom border", 0, 63, W, 1, T["border"])
for i, c in enumerate(["#FF5F57", "#FEBC2E", "#28C840"]):
    add("node.create", {"type": "ellipse", "parent": bar, "x": 16 + i * 20, "y": 10, "width": 12, "height": 12,
                        "props": {"fill": c}, "select": False})
# Wordmark: a 3x2 mini tile grid + name
mark = frame(bar, "wordmark", 16, 30, 140, 24)
for i, c in enumerate([TRACK[7], TRACK[6], TRACK[3], TRACK[0], TRACK[10], TRACK[2]]):
    rect(mark, "mark tile", (i % 3) * 7, 3 + (i // 3) * 9, 6, 8, c, 1.5)
if ":" in NAME:  # e.g. a name with a colon: the colon in the accent colour, as in the app
    head, tail = NAME.split(":", 1)
    hw = len(head) * 11.2
    text(mark, head, 28, 1, 17, "Bold", T["text"])
    text(mark, ":", 28 + hw, 1, 17, "Bold", T["accent"])
    text(mark, tail, 28 + hw + 6, 1, 17, "Bold", T["text"])
else:
    text(mark, NAME, 28, 1, 17, "Bold", T["text"], letterSpacing="-1%")

# Mode + view groups (SoundCraft toolbar groups)
g1 = frame(bar, "mode group", 170, 18, 176, 34, T["group"], 4, stroke=T["border"])
key(g1, "Perform", 4, 4, 82, on=True)
key(g1, "Edit", 90, 4, 82)
g2 = frame(bar, "view group", 354, 18, 140, 34, T["group"], 4, stroke=T["border"])
key(g2, "Grid", 4, 4, 64, on=True)
key(g2, "List", 72, 4, 64)
g3 = frame(bar, "add group", 502, 18, 128, 34, T["group"], 4, stroke=T["border"])
key(g3, "+  Add Sounds", 4, 4, 120)

# Counter: last-started cue, remaining time, green on black
cnt = frame(bar, "counter", 652, 10, 232, 46, T["counterBg"], 4, stroke=T["borderLight"])
text(cnt, "INTRO THEME", 12, 6, 10, "SemiBold", T["textDim"], letterSpacing="8%")
text(cnt, "2 PLAYING", 0, 6, 10, "SemiBold", T["textDim"], w=220, align="right", letterSpacing="8%")
text(cnt, "−0:23.4", 12, 18, 22, "SemiBold", T["counterText"], tnum=True)
text(cnt, "0:06.0 / 0:30.0", 0, 25, 12, "Regular", "#3E8A4A", w=220, align="right", tnum=True)

# Master meter (stereo, continuous fill, SoundCraft zones) + volume
mg = frame(bar, "master group", 900, 10, 220, 46, T["group"], 4, stroke=T["border"])
text(mg, "MASTER", 10, 6, 10, "SemiBold", T["textDim"], letterSpacing="8%")
for ch, level in enumerate([0.78, 0.71]):
    y = 22 + ch * 8
    rect(mg, "meter bg", 10, y, 150, 5, T["meterOff"], 1)
    gw, yw = 150 * 0.70, 150 * 0.85
    rect(mg, "meter green", 10, y, min(level * 150, gw), 5, T["meterGreen"], 1)
    if level * 150 > gw:
        rect(mg, "meter yellow", 10 + gw, y, min(level * 150, yw) - gw, 5, T["meterYellow"])
    rect(mg, "meter red zone (dim)", 10 + yw, y, 150 - yw, 5, mul(T["meterRed"], 0.25))
    rect(mg, "peak hold", 10 + level * 150 + 6, y, 2, 5, T["meterYellow"])
text(mg, "−6.2", 166, 21, 12, "Regular", T["text"], tnum=True)
key(bar, "■  Stop All", 1132, 18, 112, 34, fill=T["stop"], color="#FFD6D2")
add("node.create", {"type": "ellipse", "parent": bar, "x": 1252, "y": 29, "width": 12, "height": 12,
                    "props": {"fill": T["textDim"]}, "select": False})

# Sidebar: sets
side = frame(win, "sets sidebar", 0, 64, 208, H - 64 - 26, T["panel2"])
rect(side, "sidebar border", 207, 0, 1, H - 90, T["border"])
text(side, "SETS", 16, 14, 10, "SemiBold", T["textDim"], letterSpacing="10%")
for i, (name, n, on, playing) in enumerate([("Game Night", 12, True, True), ("Theatre Cues", 6, False, False),
                                            ("Podcast Stingers", 9, False, False), ("Wedding", 18, False, False)]):
    row = frame(side, f"set {name}", 8, 36 + i * 32, 192, 28, T["selected"] if on else None, 4)
    if playing:
        add("node.create", {"type": "ellipse", "parent": row, "x": 10, "y": 10, "width": 8, "height": 8,
                            "props": {"fill": T["counterText"]}, "select": False})
    text(row, name, 26, 6, 13, "SemiBold" if on else "Regular", T["text"])
    text(row, str(n), 0, 7, 11, "Regular", T["textDim"], w=182, align="right", tnum=True)
key(side, "+  New Set", 12, H - 64 - 26 - 42, 104)

# Grid of clip-style tiles
grid = frame(win, "grid", 208, 64, W - 208, H - 64 - 26, T["window"])
tiles = [
    ("Intro Theme", "1", 0, "playing", 0.21, "0:30.0", "−0:23"),
    ("Applause", "2", 3, None, 0, "0:06.0", ""),
    ("Rimshot", "3", 6, None, 0, "0:01.2", "↺"),
    ("Sad Trombone", "4", 10, None, 0, "0:03.5", ""),
    ("Air Horn", "5", 7, None, 0, "0:02.0", "hold"),
    ("Crickets", "6", 2, "playing", 0.62, "0:08.0", "−0:03"),
    ("Drum Roll", "7", 13, None, 0, "0:05.0", ""),
    ("Ding!", "8", 5, None, 0, "0:01.0", "↺"),
    ("Ambience Bed", "9", 1, "paused", 0.4, "0:30.0", "−0:18"),
    ("Wrong Answer", "0", 8, None, 0, "0:01.5", "↺"),
    ("Victory Fanfare", "Q", 11, None, 0, "0:07.0", ""),
    ("Outro", "W", 4, None, 0, "0:20.0", ""),
]
cols, gap, pad = 4, 14, 18
tw = (W - 208 - 2 * pad - (cols - 1) * gap) / cols
th = 150
for i, (name, hk, ci, state, prog, dur, extra) in enumerate(tiles):
    c = TRACK[ci]
    x, y = pad + (i % cols) * (tw + gap), pad + (i // cols) * (th + gap)
    outline = {"playing": T["counterText"], "paused": T["solo"]}.get(state)
    tile = frame(grid, f"tile {name}", x, y, tw, th, mul(c, 0.34), 6,
                 stroke=outline or "#0A0A0A", strokeWeight=2 if outline else 1)
    # name bar
    nb = frame(tile, "name bar", 0, 0, tw, 28, mul(c, 0.9))
    kc = frame(nb, "hotkey", 6, 5, 18, 18, "#0C0C0C59", 3)
    text(kc, hk, 0, 1, 11, "Bold", "#FFFFFF", w=18, align="center")
    text(nb, name, 30, 6, 13, "SemiBold", "#0C0C0C")
    if extra in ("↺", "hold"):
        text(nb, "RESTART" if extra == "↺" else "HOLD", 0, 8, 9, "Bold", "#0C0C0C99", w=tw - 8, align="right",
             letterSpacing="8%")
    # waveform: dim full, bright played part, playhead
    wy, wh = 36, 84
    full, _ = waveform_path(tw - 16, wh, seed=i * 7 + 3)
    vector(tile, "waveform", 8, wy, tw - 16, wh, full, tint(c, 0.35) + ("8C" if prog else "D9"))
    if prog:
        played, px = waveform_path(tw - 16, wh, seed=i * 7 + 3, upto=prog)
        vector(tile, "waveform (played)", 8, wy, max(px, 1), wh, played, tint(c, 0.75))
        rect(tile, "progress wash", 0, 28, 8 + px, th - 28, "#FFFFFF14")
        rect(tile, "playhead", 8 + px, 30, 1, th - 32, T["playhead"])
    # footer
    icon = {"playing": "▶", "paused": "❚❚"}.get(state, "")
    if icon:
        text(tile, icon, 10, th - 24, 11, "Bold", outline)
    time = extra if extra.startswith("−") else dur
    text(tile, time, 0, th - 25, 13, "SemiBold", "#FFFFFF" if state else tint(c, 0.6), w=tw - 10, align="right",
         tnum=True)

# Add slot
i = len(tiles)
x, y = pad + (i % cols) * (tw + gap), pad + (i // cols) * (th + gap)
slot = frame(grid, "add slot", x, y, tw, th, None, 6, stroke=T["borderLight"], dashes=[6, 4])
text(slot, "+  Add Sounds", 0, th / 2 - 9, 13, "Regular", T["textDim"], w=tw, align="center")

# Status bar
sb = frame(win, "status bar", 0, H - 26, W, 26, T["bar"])
rect(sb, "status border", 0, 0, W, 1, T["border"])
add("node.create", {"type": "ellipse", "parent": sb, "x": 14, "y": 9, "width": 8, "height": 8,
                    "props": {"fill": T["counterText"]}, "select": False})
text(sb, "2 playing · 1 paused", 28, 5, 12, "Regular", T["text"])
text(sb, "Output: MacBook Pro Speakers · 48 kHz", 0, 5, 12, "Regular", T["textDim"], w=W - 14, align="right")

json.dump(steps, open(OUT, "w"), indent=1)
print(f"{len(steps)} steps -> {OUT}")
