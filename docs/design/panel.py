#!/usr/bin/env python3
"""Generate a FigCraft mockup of the Panel theme: a retro sci-fi control-panel look, as an
original composition (no existing screen is
copied). Black ground; coloured elbows framing the window; stacked colour blocks; pill
buttons; tall condensed uppercase labels in black, set bottom-right; numeric readouts.

The mockup uses macOS's Helvetica Neue Condensed. The app would bundle Antonio (OFL),
the usual open condensed face, because Helvetica can't be redistributed.

Usage: panel.py OUT.json TOKENS.json   (then figcraft-cli run / render, see README)
"""
import math, random, sys

import os

# figcraft_kit.py (a small helper for figcraft-cli scripts): set FIGCRAFT_KIT to its folder.
sys.path.insert(0, os.environ.get("FIGCRAFT_KIT", os.path.expanduser("~/.claude/skills/figcraft")))
from figcraft_kit import Script, tint  # noqa: E402

OUT = sys.argv[1] if len(sys.argv) > 1 else "panel.json"
TOKENS_OUT = sys.argv[2] if len(sys.argv) > 2 else "panel-tokens.json"

# A pastel-on-black palette (our own picks).
P = {
    "black": "#000000",
    "amber": "#FF9C3A",
    "peach": "#FFC999",
    "gold": "#FFCC66",
    "lilac": "#CC99CC",
    "violet": "#9977CC",
    "periwinkle": "#9999FF",
    "sky": "#99CCFF",
    "blue": "#5588DD",
    "salmon": "#E2786A",
    "red": "#DD4444",
    "tan": "#CC9966",
    "text": "#FFC999",
    "dim": "#B07A4E",
}
FONT = "Helvetica Neue"
COND, BLACK = "Condensed Bold", "Condensed Black"

s = Script(font=FONT, text_color=P["text"])
W, H = 1280, 800
COL = 176            # left column width
TOP = 34             # top bar thickness
BOT = 26             # bottom bar thickness
R_OUT, R_IN = 64, 30  # elbow radii (outer, inner)
G = 5                 # gaps between blocks


def label(parent, txt, x, y, w, size=17, color="#000000", align="right", style=COND):
    return s.text(parent, txt.upper(), x, y, size=size, style=style, color=color, w=w, align=align,
                  letterSpacing="2%")


def top_elbow(parent, x0, y0, col_w, bar_h, x_end, y_end, fill):
    """Column (x0..x0+col_w, down to y_end) swept into a top bar (to x_end)."""
    k = 0.45
    ro, ri = R_OUT, R_IN
    path = (
        f"M0 {y_end - y0} L0 {ro} C0 {ro * k} {ro * k} 0 {ro} 0 "
        f"L{x_end - x0} 0 L{x_end - x0} {bar_h} L{col_w + ri} {bar_h} "
        f"C{col_w + ri * k} {bar_h} {col_w} {bar_h + ri * (1 - k)} {col_w} {bar_h + ri} "
        f"L{col_w} {y_end - y0} Z"
    )
    return s.vector(parent, "top elbow", x0, y0, x_end - x0, y_end - y0, path, fill=fill)


def bottom_elbow(parent, x0, y_top, col_w, bar_h, x_end, y_bottom, fill):
    """Column (from y_top) swept into a bottom bar (to x_end), mirrored top_elbow."""
    k = 0.45
    ro, ri = 44, 22
    h = y_bottom - y_top
    path = (
        f"M0 0 L{col_w} 0 L{col_w} {h - bar_h - ri} "
        f"C{col_w} {h - bar_h - ri * k} {col_w + ri * k} {h - bar_h} {col_w + ri} {h - bar_h} "
        f"L{x_end - x0} {h - bar_h} L{x_end - x0} {h} L{ro} {h} "
        f"C{ro * k} {h} 0 {h - ro * k} 0 {h - ro} Z"
    )
    return s.vector(parent, "bottom elbow", x0, y_top, x_end - x0, h, path, fill=fill)


def pill(parent, txt, x, y, w, h, fill, color="#000000", size=16, align="right"):
    p = s.frame(parent, f"pill {txt}", x, y, w, h, fill=fill, radius=h / 2)
    label(p, txt, 14, h / 2 - size * 0.62, w - 28, size=size, color=color, align=align)
    return p


def waveform_path(w, h, seed, upto=None, step=2.0):
    rnd = random.Random(seed)
    n = int(w / step)
    ph = [rnd.uniform(0, 6.28) for _ in range(3)]
    fr = [rnd.uniform(0.6, 1.4), rnd.uniform(3, 5), rnd.uniform(9, 15)]
    shape = rnd.choice(["swell", "hit", "steady", "fade"])
    pts = []
    for i in range(n + 1):
        t = i / n
        if upto is not None and t > upto:
            break
        env = {"swell": math.sin(math.pi * t) ** 0.7, "hit": math.exp(-4 * t) * 0.95 + 0.05,
               "steady": 0.78 + 0.12 * math.sin(6 * t), "fade": 1 - 0.85 * t}[shape]
        wob = sum(math.sin(f * t * 6.28 + p) * a for f, p, a in zip(fr, ph, [0.35, 0.25, 0.2]))
        pts.append((i * step, max(0.05, min(1, env * (0.65 + 0.35 * wob))) * h / 2 * 0.9))
    mid = h / 2
    top = " ".join(f"L{x:.1f} {mid - a:.1f}" for x, a in pts)
    bot = " ".join(f"L{x:.1f} {mid + a:.1f}" for x, a in reversed(pts))
    return f"M0 {mid:.1f} {top} {bot} Z", (pts[-1][0] if pts else 0)


# ── Tokens: the Panel palette as FigCraft variables ───────────────────────────────────
s.collection("Panel", ["Default"])
for k, v in P.items():
    s.variable("Panel", k, {"Default": v})
s.export_variables(["Panel"], "json", path=TOKENS_OUT)

# ── Window ────────────────────────────────────────────────────────────────────────────
win = s.frame(None, "LeadStinger — Panel theme", 0, 0, W, H, fill=P["black"], radius=10)

# Top elbow, a gap holding the title, then an end cap.
top_elbow(win, 8, 8, COL, TOP, 780, 214, P["amber"])
label(win, "Sets", 16, 178, COL - 24, size=24, style=BLACK)
label(win, "01-2026", 16, 152, COL - 24, size=12)
s.text(win, "LEADSTINGER", 792, 4, size=43, style=BLACK, color=P["amber"], letterSpacing="1%")
s.text(win, "SOUND CUE CONTROL", 792, 52, size=13, style=COND, color=P["dim"], letterSpacing="6%")
s.frame(win, "top cap", 1052, 8, W - 1052 - 8, TOP, fill=P["peach"], radius=[0, TOP / 2, TOP / 2, 0])

# Left column: set blocks under the elbow.
sets = [("Podcast", 12, P["peach"], True), ("Theatre Cues", 6, P["lilac"], False),
        ("Game Night", 9, P["periwinkle"], False), ("Stingers", 18, P["tan"], False)]
y = 214 + G
for i, (name, n, c, on) in enumerate(sets):
    blk = s.frame(win, f"set {name}", 8, y, COL, 60, fill=c if on else tint(c, 0.0))
    if on:  # the selected set: a black notch on its left edge
        s.rect(blk, "selected marker", 0, 0, 10, 60, P["black"])
    label(blk, f"{i + 1:02}-{n:03}", 18, 6, COL - 28, size=12, align="left")
    label(blk, name, 18, 34, COL - 28, size=19 if on else 17)
    y += 60 + G
s.frame(win, "new set", 8, y, COL, 40, fill=P["sky"])
label(s.frame(win, "new set label", 8, y, COL, 40), "+ New Set", 10, 12, COL - 20, size=16)
y += 40 + G
bottom_elbow(win, 8, y, COL, BOT, 640, H - 8, P["violet"])

# Bottom status: text in gaps of the bar.
s.text(win, "2 PLAYING  ·  1 PAUSED", 652, H - 8 - BOT - 1, size=22, style=COND, color=P["peach"])
s.frame(win, "status mid", 880, H - 8 - BOT, 60, BOT, fill=P["lilac"])
s.text(win, "MACBOOK PRO SPEAKERS  ·  48 KHZ", 948, H - 8 - BOT + 3, size=16,
       style=COND, color=P["lilac"], w=W - 8 - 948 - 48, align="right")
s.frame(win, "status cap", W - 8 - 40, H - 8 - BOT, 40, BOT, fill=P["lilac"], radius=[0, BOT / 2, BOT / 2, 0])

# Controls row, under the top bar.
cx, cy = COL + 8 + 40, 56
pill(win, "Perform", cx, cy, 110, 40, P["amber"])
pill(win, "Edit", cx + 116, cy, 80, 40, P["lilac"])
pill(win, "Grid", cx + 212, cy, 80, 40, P["amber"])
pill(win, "List", cx + 298, cy, 80, 40, P["periwinkle"])
pill(win, "+ Add Sounds", cx + 394, cy, 140, 40, P["sky"])

# Countdown readout and master meter (in the area right of the controls, below the title).
ry = 104
s.text(win, "INTRO THEME", 792, ry, size=15, style=COND, color=P["dim"], letterSpacing="4%")
s.text(win, "−0:23.4", 792, ry + 14, size=44, style=COND, color=P["peach"], tnum=True)
s.text(win, "0:06.0 / 0:30.0", 940, ry + 34, size=17, style=COND, color=P["dim"], tnum=True)
mx = 1062
s.text(win, "MASTER  −6.2", mx, ry, size=15, style=COND, color=P["dim"], letterSpacing="4%")
segs, lit = 16, 12
for i in range(segs):
    col = P["gold"] if i < 10 else (P["amber"] if i < 13 else P["red"])
    for ch in range(2):
        s.rect(win, "meter seg", mx + i * 13, ry + 24 + ch * 16, 10, 12, col if i < lit - ch else "#2A1A0E")
pill(win, "Stop All", 1062, 56, W - 8 - 1062, 40, P["red"], size=17)

# Tiles: colour blocks with a rounded left edge, code top-left, label bottom-right.
gx, gy, gap = COL + 8 + 40, 176, 10
cols = 4
tw = (W - 8 - gx - (cols - 1) * gap) / cols
th = 128
tiles = [
    ("Intro Theme", "01", P["peach"], "playing", 0.21, ""),
    ("Applause", "02", P["lilac"], None, 0, ""),
    ("Air Horn", "03", P["red"], None, 0, "LOOP"),
    ("Sad Trombone", "04", P["periwinkle"], None, 0, "RESTART"),
    ("Phone Ring", "05", P["sky"], None, 0, "HOLD"),
    ("Rimshot", "06", P["gold"], "playing", 0.62, ""),
    ("Drum Roll", "07", P["blue"], None, 0, ""),
    ("Crickets", "08", P["tan"], None, 0, "RESTART"),
    ("Room Tone", "09", P["violet"], "paused", 0.4, "PAUSE · LOOP"),
    ("Doorbell", "00", P["salmon"], None, 0, "RESTART"),
    ("Ad Break", "Q", P["amber"], None, 0, ""),
    ("Outro", "W", P["lilac"], None, 0, ""),
]
durations = ["0:30.0", "0:06.0", "0:04.0", "0:01.2", "0:02.0", "0:08.0", "0:03.5", "0:01.0",
             "0:30.0", "0:01.5", "0:07.0", "0:20.0"]
for i, (name, key, c, state, prog, mode) in enumerate(tiles):
    x, y = gx + (i % cols) * (tw + gap), gy + (i // cols) * (th + gap)
    fill = tint(c, 0.45) if state == "playing" else (tint(c, 0.15) if state == "paused" else c)
    tile = s.frame(win, f"tile {name}", x, y, tw, th, fill=fill, radius=[th / 2.8, 0, 0, th / 2.8])
    label(tile, key, 16, 8, 60, size=22, align="left", style=BLACK)
    label(tile, mode or ("PLAYING" if state == "playing" else ""), 60, 12, tw - 72, size=12)
    full, _ = waveform_path(tw - 40, 44, seed=i * 5 + 1)
    s.vector(tile, "wave", 22, 40, tw - 40, 44, full, fill="#00000059" if state else "#000000A6")
    if prog:
        played, px = waveform_path(tw - 40, 44, seed=i * 5 + 1, upto=prog)
        s.vector(tile, "wave played", 22, 40, max(px, 1), 44, played, fill="#000000")
        s.rect(tile, "playhead", 22 + px, 36, 2, 52, "#000000")
    remaining = {"Intro Theme": "−0:23", "Rimshot": "−0:03", "Room Tone": "−0:18"}.get(name)
    label(tile, remaining or durations[i], 16, th - 30, 90, size=17, align="left")
    label(tile, name, 90, th - 32, tw - 104, size=21)

# Add slot as an outlined block.
i = len(tiles)
x, y = gx + (i % cols) * (tw + gap), gy + (i // cols) * (th + gap)
slot = s.frame(win, "add slot", x, y, tw, th, fill=None, radius=[th / 2.8, 0, 0, th / 2.8],
               stroke=P["dim"], strokeWeight=2, dashes=[8, 6])
label(slot, "+ Add Sounds", 16, th - 32, tw - 32, size=19, color=P["dim"])

s.save(OUT)
print(f"{len(s.steps)} steps -> {OUT}")
