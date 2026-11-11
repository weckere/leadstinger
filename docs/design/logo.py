#!/usr/bin/env python3
"""Generate the LeadStinger logo drafts as a FigCraft command script.

The mark is a compact audio cassette (D-257, after 0.3.0; before it D-094's broadcast cart, the
tape cartridge radio stations fired jingles and stingers from: the cassette is less
exact but far more recognisable). Drawn in a 100 x 100 plate grid, then scaled to
1024 px; each draft is one top-level frame, with a simplified 16/32 px twin under it.
The drafts come in sets:
  duster    the cart worn across a fist like a knuckle-duster (round 2)
  held      a fist holding the cart up, fingers through holes in its back end (rounds 3-14)
  power     the cart alone, as an object of power
  cart      the cart that shipped until 0.3.0 (R2 "beams")
  cassette  the cassette drafts: C1 beams (ships), C2 tilted, C3 strip
  ship      what ships, draft C1: the Linux icon (full plate), the macOS icon (Apple's
            824 px plate on a 1024 canvas), each with its 16/32 px twin, and the cassette
            on its own in colour and in one colour; scripts/icons.sh exports them
Usage: logo.py OUT.json [SET]
"""
import json, math, os, sys

OUT = sys.argv[1] if len(sys.argv) > 1 else "logo.json"
SET = sys.argv[2] if len(sys.argv) > 2 else "power"
# The front edge's waveform (WAVES, D-128); LOGO_WAVE draws another to compare, or "" the
# head slot and tape run it replaced.
WAVE = os.environ.get("LOGO_WAVE", "bars")
PX = 1024
U = PX / 100.0

# Console (dark) and Panel colours (src/theme.rs).
INK = "#1E1E1E"      # Console window
INK2 = "#161617"     # Console bar
BLUE = "#4084C4"     # Console accent
AMBER = "#FF9C3A"    # Panel amber
ORANGE = "#E6983E"   # Console orange tile
PEACH = "#FFC999"    # Panel peach
WHITE = "#F2F0EC"


# ── the FigCraft script ───────────────────────────────────────────────────────────────

class Script:
    """FigCraft command steps; each node call returns a {"$step": n} reference to use as a
    parent. Run the saved JSON with `figcraft-cli run OUT.json --out logo.figcraft`."""

    def __init__(self):
        self.steps = []

    def _node(self, type_, parent, name, x, y, w, h, props):
        self.steps.append({"command": "node.create", "params": {
            "type": type_, "parent": parent, "name": name, "x": x, "y": y, "width": w,
            "height": h, "props": props, "select": False}})
        return {"$step": len(self.steps) - 1}

    def frame(self, parent, name, x, y, w, h, fill=None, radius=0, **props):
        """A clipping frame, transparent unless `fill` (FigCraft's default is white)."""
        p = dict(props, clip=True, fills=[fill] if fill else [])
        if radius:
            p["cornerRadius"] = radius
        return self._node("frame", parent, name, x, y, w, h, p)

    def vector(self, parent, name, x, y, w, h, path, fill=None, **props):
        """A vector whose path is in the parent's coordinates. FigCraft (0.10 on) stretches
        a path's bounds to its layer's box, so the layer is placed at the path's bounds and
        the path moved to start there; `x, y, w, h` are unused."""
        p = dict(props, **({"fill": fill} if fill else {}))
        bx0, by0, bx1, by1 = path_bounds(path)
        ref = self._node("vector", parent, name, bx0, by0, max(bx1 - bx0, 0.01), max(by1 - by0, 0.01), p)
        self.steps[-1]["params"]["path"] = shift_path(path, -bx0, -by0)
        return ref

    def save(self, path):
        with open(path, "w") as f:
            json.dump(self.steps, f, indent=1)


def path_ops(path):
    """The path data this file writes (absolute M, L, A with rx = ry, Z) as (op, numbers)."""
    import re
    out = []
    for op, args in re.findall(r"([MLAZ])([^MLAZ]*)", path):
        out.append((op, [float(v) for v in args.replace(",", " ").split()]))
    return out


def path_bounds(path):
    """The exact bounds of a path: its points, and the extremes of its circular arcs."""
    xs, ys = [], []
    cur = (0.0, 0.0)
    for op, a in path_ops(path):
        if op in "ML":
            cur = (a[0], a[1])
            xs.append(a[0])
            ys.append(a[1])
        elif op == "A":
            r, large, sweep, end = a[0], a[3], a[4], (a[5], a[6])
            # The arc's centre (SVG's endpoint parameterisation, for a circle).
            (x1, y1), (x2, y2) = cur, end
            mx, my, dx, dy = (x1 + x2) / 2, (y1 + y2) / 2, (x2 - x1) / 2, (y2 - y1) / 2
            d = math.hypot(dx, dy)
            r = max(r, d)
            hgt = math.sqrt(max(r * r - d * d, 0.0))
            sign = 1 if large != sweep else -1
            cx, cy = (mx - sign * hgt * dy / d, my + sign * hgt * dx / d) if d else (x1, y1)
            a1 = math.atan2(y1 - cy, x1 - cx)
            a2 = math.atan2(y2 - cy, x2 - cx)
            span = (a2 - a1) % (2 * math.pi) if sweep else -((a1 - a2) % (2 * math.pi))
            for k in range(33):
                t = a1 + span * k / 32
                xs.append(cx + r * math.cos(t))
                ys.append(cy + r * math.sin(t))
            for q in range(4):                      # the arc's axis extremes, exactly
                t = q * math.pi / 2
                rel = ((t - a1) % (2 * math.pi)) if sweep else ((a1 - t) % (2 * math.pi))
                if rel <= abs(span):
                    xs.append(cx + r * math.cos(t))
                    ys.append(cy + r * math.sin(t))
            cur = end
            xs.append(end[0])
            ys.append(end[1])
    return min(xs), min(ys), max(xs), max(ys)


def shift_path(path, dx, dy):
    out = []
    for op, a in path_ops(path):
        if op in "ML":
            a = [a[0] + dx, a[1] + dy]
        elif op == "A":
            a = a[:5] + [a[5] + dx, a[6] + dy]
        out.append(op + " ".join(f(v) for v in a))
    return " ".join(out)


# ── geometry (plate units, transformed then scaled to px) ─────────────────────────────

class T:
    """Rotate by `deg` about (cx, cy), then translate by (dx, dy)."""

    def __init__(self, deg=0.0, cx=50.0, cy=50.0, dx=0.0, dy=0.0, s=1.0):
        self.a, self.cx, self.cy, self.dx, self.dy, self.s = math.radians(deg), cx, cy, dx, dy, s

    def __call__(self, x, y):
        c, s = math.cos(self.a), math.sin(self.a)
        x, y = x - self.cx, y - self.cy
        x, y = x * c - y * s, x * s + y * c
        x, y = x * self.s + self.cx + self.dx, y * self.s + self.cy + self.dy
        return x * U, y * U


def f(v):
    return f"{v:.2f}".rstrip("0").rstrip(".")


def rrect(t, x0, y0, x1, y1, r, ccw=False):
    """A rounded rectangle as path data (circular corners survive rotation). `ccw` winds it
    the other way, which makes it a hole in a shape drawn clockwise."""
    r = min(r, (x1 - x0) / 2, (y1 - y0) / 2)
    R = r * U * t.s
    pts = [((x0 + r, y0), None), ((x1 - r, y0), None), ((x1, y0 + r), "a"), ((x1, y1 - r), None),
           ((x1 - r, y1), "a"), ((x0 + r, y1), None), ((x0, y1 - r), "a"), ((x0, y0 + r), None),
           ((x0 + r, y0), "a")]
    if ccw:
        # The same outline backwards: each corner arc now belongs to the point before it.
        rev = list(reversed(pts))
        pts = [(rev[0][0], None)] + [(rev[i][0], rev[i - 1][1]) for i in range(1, len(rev))]
    out = []
    for i, ((x, y), kind) in enumerate(pts):
        X, Y = t(x, y)
        if i == 0:
            out.append(f"M{f(X)} {f(Y)}")
        elif kind == "a" and r > 0:
            out.append(f"A{f(R)} {f(R)} 0 0 {0 if ccw else 1} {f(X)} {f(Y)}")
        else:
            out.append(f"L{f(X)} {f(Y)}")
    return " ".join(out) + " Z"


def circle(t, cx, cy, r, cw=False):
    """A circle, counter-clockwise (a hole in a clockwise shape) unless `cw`."""
    X, Y = t(cx, cy)
    R = r * U * t.s
    sw = 1 if cw else 0
    return (f"M{f(X - R)} {f(Y)} A{f(R)} {f(R)} 0 1 {sw} {f(X + R)} {f(Y)} "
            f"A{f(R)} {f(R)} 0 1 {sw} {f(X - R)} {f(Y)} Z")


def poly(t, pts):
    P = [t(x, y) for x, y in pts]
    return "M" + " L".join(f"{f(x)} {f(y)}" for x, y in P) + " Z"


def capsule(t, x0, y0, x1, y1):
    return rrect(t, x0, y0, x1, y1, min(x1 - x0, y1 - y0) / 2)


def line_capsule(t, a, b, w):
    """A capsule of width `w` from point a to point b (a thumb, a ray)."""
    (ax, ay), (bx, by) = a, b
    L = math.hypot(bx - ax, by - ay)
    ang = math.degrees(math.atan2(by - ay, bx - ax))
    # Built unrotated at a, rotated about a, then put through t.
    x0, y0, x1, y1 = ax - w / 2, ay - w / 2, ax + L + w / 2, ay + w / 2
    path = []
    r = w / 2
    c, s = math.cos(math.radians(ang)), math.sin(math.radians(ang))

    def rot(x, y):
        x, y = x - ax, y - ay
        return t(ax + x * c - y * s, ay + x * s + y * c)

    R = r * U * t.s
    seq = [((x0 + r, y0), None), ((x1 - r, y0), None), ((x1 - r, y1), "a"), ((x0 + r, y1), None),
           ((x0 + r, y0), "a")]
    for i, ((x, y), kind) in enumerate(seq):
        X, Y = rot(x, y)
        if i == 0:
            path.append(f"M{f(X)} {f(Y)}")
        elif kind == "a":
            path.append(f"A{f(R)} {f(R)} 0 0 1 {f(X)} {f(Y)}")
        else:
            path.append(f"L{f(X)} {f(Y)}")
    return " ".join(path) + " Z"


def rays(t, cx, cy, n, r0, r1, spread, start, end):
    """Wedges fanning out from (cx, cy) between angles start..end (degrees)."""
    out = []
    for i in range(n):
        a = math.radians(start + (end - start) * (i + 0.5) / n)
        h = math.radians(spread / 2)
        p = [(cx + r0 * math.cos(a - h * r0 / r1), cy + r0 * math.sin(a - h * r0 / r1)),
             (cx + r1 * math.cos(a - h), cy + r1 * math.sin(a - h)),
             (cx + r1 * math.cos(a + h), cy + r1 * math.sin(a + h)),
             (cx + r0 * math.cos(a + h * r0 / r1), cy + r0 * math.sin(a + h * r0 / r1))]
        out.append(poly(t, p))
    return " ".join(out)


# ── the mark ──────────────────────────────────────────────────────────────────────────

def cart(t, cx=50.0, top=34.0, w=64.0, h=26.0, edge=5.0):
    """A broadcast cart lying across the fist, label face out, a little from above: its
    front edge on top (the head slot and pinch roller, with the tape running past), the
    tape pack through the window on the left, the label on the right."""
    x0, x1, bottom = cx - w / 2, cx + w / 2, top + h
    rcx, rcy, rr = x0 + h * 0.52 + 1.5, top + h / 2, h * 0.36
    lx0 = rcx + rr + 3.2
    return {
        "edge": rrect(t, x0, top - edge, x1, top + 6, 3.4),
        "body": rrect(t, x0, top, x1, bottom, 3.4),
        "slot": " ".join([rrect(t, x0 + 6, top - edge + 1.3, x0 + 12, top - 1.1, 0.8),
                          rrect(t, cx - 10, top - edge + 1.3, cx + 8, top - 1.1, 0.8),
                          rrect(t, x1 - 15, top - edge + 1.3, x1 - 6, top - 1.1, 0.8)]),
        "tape_run": rrect(t, x0 + 6, top - edge / 2 - 0.5, x1 - 6, top - edge / 2 + 0.8, 0),
        "roller": circle(t, x1 - 9.5, top - edge / 2 + 0.2, 1.5),
        "reel": circle(t, rcx, rcy, rr),
        "hub": circle(t, rcx, rcy, rr * 0.4),
        "hub_hole": circle(t, rcx, rcy, rr * 0.17),
        "label": rrect(t, lx0, top + 3.6, x1 - 3.6, bottom - 3.6, 1.6),
        "label_lines": " ".join([rrect(t, lx0 + 3, top + 7.6, x1 - 8, top + 9.2, 0.8),
                                 rrect(t, lx0 + 3, top + 11.6, x1 - 13, top + 13.2, 0.8),
                                 rrect(t, lx0 + 3, top + 15.6, x1 - 10, top + 17.2, 0.8)]),
    }


def fist(t, cx=50.0, knuckles=20.0, base=72.0, fw=9.6, gap=1.4, thumb_y=68.0):
    """A raised fist, palm out: four fingers side by side (knuckles at the top), the palm
    under them, the thumb folded across, a wristband and the forearm off the plate."""
    total = 4 * fw + 3 * gap
    x0 = cx - total / 2
    lift = [2.0, 0.0, 0.6, 2.6]
    fingers, rings = [], []
    x = x0
    for i in range(4):
        fingers.append(capsule(t, x, knuckles + lift[i], x + fw, base))
        x += fw + gap
    return {
        "fingers": " ".join(fingers),
        "palm": rrect(t, x0, base - 14, x0 + total, base + 10, 8),
        "thumb": line_capsule(t, (x0 + 2.5, thumb_y + 3.5), (cx - 2.0, thumb_y - 1.0), 9.0),
        "cuff": rrect(t, cx - 19, base + 8.5, cx + 19, base + 16, 1.6),
        "arm": poly(t, [(cx - 17.5, base + 15), (cx + 17.5, base + 15), (cx + 20, 112), (cx - 20, 112)]),
        "x0": x0, "fw": fw, "gap": gap,
    }


def plate(s, frame, style):
    """The rounded plate, clipping what's drawn on it (the rays, the arm). Full size, or
    with `inset` (macOS: an 824 px plate on the 1024 px canvas, corners smoothed)."""
    if style.get("plate") is None:
        return frame
    i = style.get("inset", 0)
    if not i:
        return s.frame(frame, "plate", 0, 0, PX, PX, fill=style["plate"], radius=PX * 0.1875)
    return s.frame(frame, "plate", i, i, PX - 2 * i, PX - 2 * i, fill=style["plate"],
                   radius=(PX - 2 * i) * 0.225, cornerSmoothing=0.6)


def vec(s, parent, name, path, fill=None, stroke=None, weight=0, **props):
    p = dict(props)
    if stroke:
        p.update(stroke=stroke, strokeWeight=weight, strokeAlign="center", strokeJoin="round",
                 strokeCap="round")
        if not fill:
            p["fills"] = []
    return s.vector(parent, name, 0, 0, PX, PX, path, fill=fill, **p)


def draw_cart(s, frame, k, c, small):
    vec(s, frame, "edge", k["edge"], fill=c["edge"])
    if not small and k.get("wave"):
        vec(s, frame, "wave", k["wave"], fill=c["slot"])
    elif not small:
        vec(s, frame, "slot", k["slot"], fill=c["slot"])
        vec(s, frame, "tape run", k["tape_run"], fill=c["tape"])
        vec(s, frame, "roller", k["roller"], fill=c["edge"])
    vec(s, frame, "body", k["body"], fill=c["cart"])
    vec(s, frame, "label", k["label"], fill=c["label"])
    if not small:
        vec(s, frame, "label lines", k["label_lines"], fill=c["cart"])
    vec(s, frame, "reel", k["reel"], fill=c["tape"])
    vec(s, frame, "hub", k["hub"], fill=c.get("hub", c["label"]))
    if not small:
        vec(s, frame, "hub hole", k["hub_hole"], fill=c["tape"])


def knuckles(s, frame, t, c, rings=False):
    """The cart worn across the fist like a knuckle-duster: knuckles above it, the curled
    fingers and thumb below. With `rings`, the fingers come through four rings on the
    cart's front edge, like a real duster's."""
    small = c.get("small")
    gap = (1.8 if small else 1.2) * U
    h = fist(t)
    k = cart(t, top=40.0 if rings else 34.0)
    if not small:
        vec(s, frame, "rays", rays(t, 50, 44, 9, 34, 80, 9, 180, 360), fill=c["rays"],
            opacity=c.get("rays_a", 1))
    vec(s, frame, "arm", h["arm"], fill=c["hand"])
    vec(s, frame, "cuff", h["cuff"], fill=c["cuff"])
    vec(s, frame, "palm", h["palm"], fill=c["hand"])
    vec(s, frame, "fingers", h["fingers"], fill=c["hand"], stroke=c["plate_ink"], weight=gap)
    draw_cart(s, frame, k, c, small)
    if rings:
        step = h["fw"] + h["gap"]
        bands = [rrect(t, h["x0"] + i * step - 0.9, 28.5, h["x0"] + i * step + h["fw"] + 0.9, 35.4, 2.4)
                 for i in range(4)]
        vec(s, frame, "rings", " ".join(bands), fill=c["cart"], stroke=c["plate_ink"], weight=gap)
    vec(s, frame, "thumb", h["thumb"], fill=c["hand"], stroke=c["plate_ink"], weight=gap)


def duster(s, frame, t, c):
    knuckles(s, frame, t, c)


def ringed(s, frame, t, c):
    knuckles(s, frame, t, c, rings=True)


HOLES = (52.5, 63.0)
ARC = 2.2   # how much lower the outer holes sit than the middle ones (a duster's arch)


class Turned:
    """`t`, after turning by `deg` about (cx, cy) in plate units: for one tilted hole."""

    def __init__(self, t, deg, cx, cy):
        self.t, self.s, self.cx, self.cy = t, t.s, cx, cy
        self.c, self.sn = math.cos(math.radians(deg)), math.sin(math.radians(deg))

    def __call__(self, x, y):
        x, y = x - self.cx, y - self.cy
        return self.t(self.cx + x * self.c - y * self.sn, self.cy + x * self.sn + y * self.c)


def arc(x, cx=50.0, half=14.85):
    """The holes' arch at x: (drop below the middle, tilt in degrees)."""
    k = ARC / half ** 2
    d = x - cx
    return k * d * d, math.degrees(math.atan(2 * k * d)) * 0.6


def upright(t, cx=50.0, top=12.0, bottom=67.0, w=46.0, edge=6.0, holes=HOLES,
            fw=8.6, gap=1.3):
    """A broadcast cart standing up, label face out, a little from above: its front edge on
    top (the head slot and pinch roller, the tape running past), the tape pack through the
    window, the label, and four finger holes through the back end, so it's held like a
    knuckle-duster. `body` is one path with the holes cut out. With `holes=None` it is a
    plain cart, its label running down to near the bottom."""
    x0, x1 = cx - w / 2, cx + w / 2
    rcy, rr = top + 15.5, 11.5
    total = 4 * fw + 3 * gap
    hx0 = cx - total / 2
    hole_paths = []
    label_bottom = holes[0] - 3.0 if holes else bottom - 4.0
    for i in range(4 if holes else 0):
        hx = hx0 + i * (fw + gap) + fw / 2
        dy, tilt = arc(hx, cx)
        ht = Turned(t, tilt, hx, (holes[0] + holes[1]) / 2 + dy)
        hole_paths.append(rrect(ht, hx - fw / 2 - 0.5, holes[0] + dy, hx + fw / 2 + 0.5, holes[1] + dy,
                                4.8, ccw=True))
    return {
        "edge": rrect(t, x0, top - edge, x1, top + 6, 3.8),
        "body": " ".join([rrect(t, x0, top, x1, bottom, 3.8)] + hole_paths),
        "slot": " ".join([rrect(t, x0 + 4.5, top - edge + 1.4, x0 + 10, top - 1.2, 0.8),
                          rrect(t, cx - 9, top - edge + 1.4, cx + 7, top - 1.2, 0.8),
                          rrect(t, x1 - 13, top - edge + 1.4, x1 - 4.5, top - 1.2, 0.8)]),
        "tape_run": rrect(t, x0 + 4.5, top - edge / 2 - 0.5, x1 - 4.5, top - edge / 2 + 0.8, 0),
        "roller": circle(t, x1 - 8.5, top - edge / 2 + 0.2, 1.6),
        "wave": wave(t, x0 + 4.5, x1 - 4.5, top - edge / 2, 2.6, WAVE) if WAVE else None,
        "reel": circle(t, cx, rcy, rr),
        "hub": circle(t, cx, rcy, rr * 0.4),
        "hub_hole": circle(t, cx, rcy, rr * 0.17),
        "label": rrect(t, x0 + 4, rcy + rr + 2.6, x1 - 4, label_bottom, 1.6),
        "label_lines": " ".join([rrect(t, x0 + 7, rcy + rr + 5.6, x1 - 11, rcy + rr + 7.1, 0.75),
                                 rrect(t, x0 + 7, rcy + rr + 8.6, x1 - 17, rcy + rr + 10.1, 0.75)]
                                + ([rrect(t, x0 + 7, rcy + rr + 11.6, x1 - 14, rcy + rr + 13.1, 0.75)]
                                   if not holes else [])),
        "hx0": hx0, "fw": fw, "gap": gap, "rcy": rcy, "rr": rr, "holes": " ".join(hole_paths),
        "outline": rrect(t, x0, top - edge, x1, bottom, 3.8),
        "glyph_label": rrect(t, x0 + 4, rcy + rr + 2.6, x1 - 4, label_bottom, 1.6, ccw=True),
    }


# The waveform along the cart's front edge, in place of the head slot and tape run (D-128):
# each style is the bars' heights (0..1 of the edge's half height), or "envelope" for a
# filled outline through those heights. "bars" ships; "fine" and "envelope" were the
# alternatives considered (fine smears to grey at 64 px).
WAVES = {
    "bars": [0.42, 0.78, 1.00, 0.58, 0.90, 0.48, 0.74, 0.40, 0.60, 0.34, 0.26],
    "fine": [0.30, 0.62, 0.96, 1.00, 0.56, 0.84, 0.70, 0.92, 0.50, 0.76, 0.44, 0.64, 0.38, 0.54,
             0.32, 0.42, 0.26, 0.30, 0.18],
    "envelope": [0.30, 0.92, 0.40, 1.00, 0.52, 0.86, 0.30, 0.78, 0.44, 0.70, 0.26, 0.62, 0.36,
                 0.54, 0.22, 0.46, 0.30, 0.38, 0.16, 0.30, 0.12],
}


def wave(t, x0, x1, cy, half, style):
    """A waveform from x0 to x1 about the line y = cy, at most `half` above and below it."""
    amps = WAVES[style]
    n = len(amps)
    if style == "envelope":
        # A smooth outline through the heights (Catmull-Rom), mirrored about the centre
        # line and pinched to a point at each end, like the Panel tiles' waveforms.
        pts = [(x0, 0.0)] + [(x0 + (x1 - x0) * (i + 0.5) / n, a) for i, a in enumerate(amps)] + [(x1, 0.0)]
        curve = []
        for i in range(len(pts) - 1):
            p0, p1, p2, p3 = pts[max(i - 1, 0)], pts[i], pts[i + 1], pts[min(i + 2, len(pts) - 1)]
            for j in range(6):
                u = j / 6
                curve.append(tuple(0.5 * (2 * b + (c - a) * u + (2 * a - 5 * b + 4 * c - d) * u * u
                                          + (3 * b - a - 3 * c + d) * u ** 3)
                                   for a, b, c, d in zip(p0, p1, p2, p3)))
        curve.append(pts[-1])
        top = [(x, cy - max(a, 0.0) * half - 0.3) for x, a in curve]
        bottom = [(x, cy + max(a, 0.0) * half + 0.3) for x, a in reversed(curve)]
        return poly(t, top + bottom)
    pitch = (x1 - x0) / n
    w = pitch * (0.56 if n < 14 else 0.5)
    bars = [capsule(t, x0 + i * pitch + (pitch - w) / 2, cy - max(a * half, w / 2),
                    x0 + i * pitch + (pitch + w) / 2, cy + max(a * half, w / 2))
            for i, a in enumerate(amps)]
    return " ".join(bars)


def polyline(t, pts):
    P = [t(x, y) for x, y in pts]
    return "M" + " L".join(f"{f(x)} {f(y)}" for x, y in P)


def cubic(p0, p1, p2, p3, n=20):
    """Points along a cubic Bezier (plate units)."""
    return [tuple((1 - u) ** 3 * a + 3 * (1 - u) ** 2 * u * b + 3 * (1 - u) * u * u * c + u ** 3 * d
                  for a, b, c, d in zip(p0, p1, p2, p3))
            for u in (i / n for i in range(n + 1))]


def quad(p0, p1, p2, n=16):
    """Points along a quadratic Bezier (plate units)."""
    return [((1 - u) ** 2 * p0[0] + 2 * (1 - u) * u * p1[0] + u * u * p2[0],
             (1 - u) ** 2 * p0[1] + 2 * (1 - u) * u * p1[1] + u * u * p2[1])
            for u in (i / n for i in range(n + 1))]


# Where the thumb is cut off and the hand's own outline takes over.
THUMB_CUT = 84.0

# Where the fingers bend and end, and how far each part closes in on the fist's middle
# (1 = straight down): the middle segment runs a little inward, the tip curls in further.
JOINT_Y, TIP_Y = 68.0, 75.5
JOINT_IN, TIP_IN = 0.9, 0.74


def finger_shape(t, k, j, tip, w0, w1, w2, n=10):
    """A finger bent at its middle joint: rounded at the knuckle (half-width w0) and the
    tip (w2), w1 wide at the joint. Its sides run straight between those, so fingers whose
    widths match their spacing sit side by side, squeezed together, never on top."""
    def unit(a, b):
        dx, dy = b[0] - a[0], b[1] - a[1]
        ln = math.hypot(dx, dy)
        return dx / ln, dy / ln
    d1, d2 = unit(k, j), unit(j, tip)
    n1, n2 = (-d1[1], d1[0]), (-d2[1], d2[0])
    m = (n1[0] + n2[0], n1[1] + n2[1])
    ml = math.hypot(*m)
    m = (m[0] / ml, m[1] / ml)
    m = (m[0] / (m[0] * n1[0] + m[1] * n1[1]), m[1] / (m[0] * n1[0] + m[1] * n1[1]))  # mitre
    pts = []
    for i in range(n + 1):                     # over the knuckle, right side to left
        a = math.pi * i / n
        pts.append((k[0] + w0 * (-math.cos(a) * n1[0] - math.sin(a) * d1[0]),
                    k[1] + w0 * (-math.cos(a) * n1[1] - math.sin(a) * d1[1])))
    pts.append((j[0] + w1 * m[0], j[1] + w1 * m[1]))
    for i in range(n + 1):                     # round the tip, left side to right
        a = math.pi * i / n
        pts.append((tip[0] + w2 * (math.cos(a) * n2[0] + math.sin(a) * d2[0]),
                    tip[1] + w2 * (math.cos(a) * n2[1] + math.sin(a) * d2[1])))
    pts.append((j[0] - w1 * m[0], j[1] - w1 * m[1]))
    return poly(t, pts)


def held(s, frame, t, c):
    """The cart held up like a knuckle-duster: each finger comes out of its hole and curls
    down in front of the cart, hiding the bar below the holes (that sits in the palm). The
    fingers bend at the middle joint (a crease) and their tips turn in toward the fist's
    middle. The thumb comes up the side of the fist, bends at two knuckles and lies across
    the first finger. Below the fist the hand narrows to the wrist, then the forearm."""
    small = c.get("small")
    gap = (1.8 if small else 1.2) * U
    k = upright(t)
    fw, top = k["fw"], HOLES[0] + 2.2
    step = fw + k["gap"]
    xs = [k["hx0"] + i * step + fw / 2 for i in range(4)]     # finger centres at the holes
    paths = {}
    for i, x in enumerate(xs):
        d = x - 50
        knuckle = (x, top + arc(x)[0] + fw / 2)
        joint = (50 + d * JOINT_IN, JOINT_Y)
        tip = (50 + d * TIP_IN, TIP_Y)
        # The crease across the finger at the joint, square to the middle segment.
        ang = math.atan2(joint[1] - knuckle[1], joint[0] - knuckle[0]) + math.pi / 2
        hl = fw * 0.26
        crease = [(joint[0] - hl * math.cos(ang), joint[1] - hl * math.sin(ang)),
                  (joint[0] + hl * math.cos(ang), joint[1] + hl * math.sin(ang))]
        # Clear of each other at the holes, then as wide as their spacing: squeezed together.
        shape = finger_shape(t, knuckle, joint, tip, fw / 2, step * JOINT_IN / 2, step * TIP_IN / 2)
        paths[i] = (shape, polyline(t, crease))
    x0, x1 = k["hx0"], k["hx0"] + 4 * fw + 3 * k["gap"]
    # The back of the fist, narrowing to the wrist, and the forearm running off the plate.
    thumb = [(x0 + 3.0, 86.0), (x0 - 1.2, 76.0), (x0 + 2.8, 70.2), (x0 + 10.0, 71.4)]
    tw = 8.8
    # The thumb's white edge on the outside of its lower segment, where it is cut off: the
    # hand's outline starts exactly there, heading the same way, then bends smoothly into
    # the wrist and the forearm. The right side does the same from the little finger.
    (bx, by), (kx, ky) = thumb[0], thumb[1]
    ln = math.hypot(bx - kx, by - ky)
    tdx, tdy = (bx - kx) / ln, (by - ky) / ln
    nx, ny = -tdy, tdx                                  # the outward normal (to the left)
    u = (THUMB_CUT - ky - tw / 2 * ny) / (by - ky)
    join = (kx + u * (bx - kx) + tw / 2 * nx, THUMB_CUT)
    hand = poly(t, [(x0, 62.0), (x0, 72.0), (x0 - 2.5, 80.0)]
                + cubic(join, (join[0] + 4 * tdx, join[1] + 4 * tdy), (36.7, 91.5), (36.5, 96.0))
                + [(35.6, 112.0), (64.4, 112.0)]
                + cubic((63.5, 96.0), (63.3, 91.5), (x1, 86.0), (x1, 77.0))
                + [(x1, 62.0)])
    if not small:
        vec(s, frame, "rays", rays(t, 50, 34, 9, 32, 80, 9, 180, 360), fill=c["rays"],
            opacity=c.get("rays_a", 1))
    vec(s, frame, "hand", hand, fill=c["hand"])
    draw_cart(s, frame, k, c, small)
    for i in range(4):
        finger, crease = paths[i]
        vec(s, frame, f"finger {i + 1}", finger, fill=c["hand"], stroke=c["plate_ink"], weight=gap)
        if not small:
            vec(s, frame, f"finger {i + 1} crease", crease, stroke=c["plate_ink"], weight=0.9 * U)
    # The thumb is cut off where it joins the hand (THUMB_CUT): below that it is the hand.
    tc = s.frame(frame, "thumb", 0, 0, PX, t(join[0], THUMB_CUT)[1])
    vec(s, tc, "thumb outline", polyline(t, thumb), stroke=c["plate_ink"], weight=tw * U + 2 * gap)
    vec(s, tc, "thumb fill", polyline(t, thumb), stroke=c["hand"], weight=tw * U)
    if not small:
        # Two palm creases, as light as the finger creases: one round the ball of the
        # thumb, one in from the little finger's side.
        creases = [quad((43.5, 80.8), (40.3, 85.0), (41.6, 90.6)),
                   quad((66.6, 81.6), (60.0, 84.6), (53.2, 83.6))]
        vec(s, frame, "palm creases", " ".join(polyline(t, q) for q in creases),
            stroke=c["plate_ink"], weight=0.9 * U)


def cart_glyph(t):
    """The cart alone as ONE path: body and front edge, with the tape window, the label
    and the finger holes cut out and the hub left standing. For one colour (menu bar,
    small buttons)."""
    k = upright(t)
    return " ".join([k["outline"], k["holes"],
                     circle(t, 50, k["rcy"], k["rr"]), circle(t, 50, k["rcy"], k["rr"] * 0.42, cw=True),
                     k["glyph_label"]])


def cart_only(s, frame, t, c):
    """The cart alone, in colour (no plate, no hand)."""
    draw_cart(s, frame, upright(t), c, small=True)


def cart_mono(s, frame, t, c):
    vec(s, frame, "cart", cart_glyph(t), fill=c["ink"])


A_COLOURS = dict(plate=INK, rays=BLUE, rays_a=0.55, hand=WHITE, cuff=BLUE, cart=AMBER,
                 edge="#C46E1E", slot=INK, label=PEACH, tape="#6A3E1E", plate_ink=INK)
MONO = dict(plate=AMBER, rays=INK, rays_a=0.22, hand=INK, cuff=AMBER, cart=INK, edge=INK,
            slot=AMBER, label=AMBER, tape=AMBER, hub=INK, plate_ink=AMBER)

def sparkle(t, cx, cy, r):
    """A four-pointed glint."""
    q = r * 0.26
    return poly(t, [(cx, cy - r), (cx + q, cy - q), (cx + r, cy), (cx + q, cy + q), (cx, cy + r),
                    (cx - q, cy + q), (cx - r, cy), (cx - q, cy - q)])


PLAIN = dict(top=12.0, bottom=60.0, holes=None)
MIDDLE = (50.0, 33.0)          # the plain cart's middle, before it is placed on the plate


def power(s, frame, t, c, burst=(16, 22, 80, 7, 0, 360), glow=True, glints=(), holes=False):
    """The cart alone, lit up like an object of power: a burst of rays and a glow behind it,
    and glints round it. With `holes`, it is the knuckle-duster cart."""
    small = c.get("small")
    shape = dict(PLAIN, holes=HOLES, bottom=67.0) if holes else PLAIN
    k = upright(t, **shape)
    mx, my = 50.0, (shape["top"] - 6.0 + shape["bottom"]) / 2
    if glow:
        X, Y = t(mx, my)
        R = 46 * U * t.s
        s._node("ellipse", frame, "glow", X - R, Y - R, 2 * R, 2 * R, {"fills": [
            {"type": "radialGradient", "stops": [c["glow"] + "AA", c["glow"] + "33", c["glow"] + "00"]}]})
    if not small and burst:
        n, r0, r1, spread, a0, a1 = burst
        vec(s, frame, "rays", rays(t, mx, my, n, r0, r1, spread, a0, a1), fill=c["rays"],
            opacity=c.get("rays_a", 1))
    draw_cart(s, frame, k, c, small)
    if not small:
        for i, (gx, gy, gr) in enumerate(glints):
            vec(s, frame, f"glint {i + 1}", sparkle(t, gx, gy, gr), fill=c.get("glint", WHITE))


def burst(s, frame, t, c):
    power(s, frame, t, c)


def beams(s, frame, t, c):
    power(s, frame, t, c, burst=(7, 24, 85, 12, 200, 340),
          glints=[(20, 18, 4.5), (82, 30, 3.2), (24, 66, 2.6)])


def hover(s, frame, t, c):
    power(s, frame, t, c, burst=(12, 24, 80, 6, 0, 360),
          glints=[(19, 22, 4.6), (80, 14, 3.4), (84, 62, 2.8), (16, 58, 2.4)])


def duster_cart(s, frame, t, c):
    power(s, frame, t, c, holes=True, glints=[(18, 20, 4.2), (82, 64, 3.0)])


# Each draft: name, drawing, pose, colours[, colours of its 16/32 px twin].
ON_PLATE = T(cy=MIDDLE[1], dy=50 - MIDDLE[1], s=1.08)
POWER_COLOURS = dict(A_COLOURS, glow=AMBER, rays=BLUE, rays_a=0.6)
SETS = {
    "duster": [
        ("P1 duster", duster, T(), A_COLOURS),
        ("P2 rings", ringed, T(), A_COLOURS),
        ("P3 punch", duster, T(-14, 50, 66, dx=3), A_COLOURS),
        ("P4 mono", duster, T(), MONO),
    ],
    "held": [
        ("Q1 held", held, T(), A_COLOURS),
        ("Q2 tilted", held, T(-12, 50, 72, dx=2), A_COLOURS),
        ("Q3 mono", held, T(), MONO),
        ("Cart", cart_only, T(cy=38, s=1.5, dy=12), dict(A_COLOURS, plate=None)),
        ("Cart mono", cart_mono, T(cy=38, s=1.5, dy=12), dict(plate=None, ink=AMBER)),
    ],
    "power": [
        ("R1 burst", burst, ON_PLATE, POWER_COLOURS),
        ("R2 beams", beams, ON_PLATE, dict(POWER_COLOURS, rays=AMBER, rays_a=0.45)),
        ("R3 hover", hover, T(-10, *MIDDLE, dy=50 - MIDDLE[1], s=1.02), POWER_COLOURS),
        ("R4 duster cart", duster_cart, T(cy=36.5, dy=13.5, s=1.02), POWER_COLOURS),
        ("R5 mono", burst, ON_PLATE, dict(MONO, glow=INK, rays=INK, rays_a=0.25, glint=INK)),
    ],
}


def plain_cart(s, frame, t, c):
    """The cart alone, flat, no glow: for small buttons and the toolbar."""
    draw_cart(s, frame, upright(t, **PLAIN), c, small=c.get("small", False))


def plain_glyph(s, frame, t, c):
    """The cart alone in one colour: one path, the tape window and label cut out."""
    k = upright(t, **PLAIN)
    vec(s, frame, "cart", " ".join([k["outline"], circle(t, 50, k["rcy"], k["rr"]),
                                    circle(t, 50, k["rcy"], k["rr"] * 0.42, cw=True),
                                    k["glyph_label"]]), fill=c["ink"])


def fit(inset):
    """R2's placement, shrunk onto a plate inset by `inset` px."""
    k = (PX - 2 * inset) / PX
    return T(cy=MIDDLE[1], s=1.08 * k, dx=50 * k - 50, dy=50 * k - MIDDLE[1])


R2 = dict(POWER_COLOURS, rays=AMBER, rays_a=0.45)
GLYPH_FIT = T(cy=MIDDLE[1], dy=50 - MIDDLE[1], s=1.6)
SMALL = T(cy=30, s=1.1)


# ── the cassette (D-257, D-094's successor) ──────────────────────────────────────
#
# A generic compact cassette seen square on, landscape: the shell, the label across its
# face with the D-128 waveform along its top, the tape window with two reel hubs and the
# tape wound round them, the trapezoid along the bottom edge with its holes, and the
# screws. Drawn about the plate's middle, 80 wide and 51 high (a real cassette's 1.57:1).

CX0, CX1, CY0, CY1 = 10.0, 90.0, 24.0, 75.0     # the shell
HUBS = (36.5, 63.5)                             # the hubs' x; they sit on y = WIN_Y
WIN_Y = 47.0


def trap_pts(inset=0.0):
    """The trapezoid along the bottom edge, clockwise; `inset` shrinks it (not its foot)."""
    return [(29.0 + inset, 63.0 + inset), (71.0 - inset, 63.0 + inset),
            (76.0 - inset * 1.4, CY1), (24.0 + inset * 1.4, CY1)]


def tooth(t, cx, cy, deg, r0, r1, w):
    """One spline tooth of a hub, pointing in from radius r1 to r0."""
    a = math.radians(deg)
    c, s_ = math.cos(a), math.sin(a)
    n = (-s_ * w / 2, c * w / 2)
    return poly(t, [(cx + r0 * c - n[0], cy + r0 * s_ - n[1]), (cx + r1 * c - n[0], cy + r1 * s_ - n[1]),
                    (cx + r1 * c + n[0], cy + r1 * s_ + n[1]), (cx + r0 * c + n[0], cy + r0 * s_ + n[1])])


def cassette(t, small=False, label="peach"):
    """The cassette's parts as paths. `small` is the 16/32 px twin: a taller window and
    bigger hubs, nothing finer. `label` "peach" is a light label over the whole face;
    "strip" is a dark band across the top only, the waveform lit in amber on it."""
    win = (22.0, 34.0, 78.0, 59.0) if small else (28.0, 39.0, 72.0, 55.0)
    hub_r = 6.8 if small else 4.4
    k = {
        "shell": rrect(t, CX0, CY0, CX1, CY1, 4.5),
        "window": rrect(t, *win, (win[3] - win[1]) / 2),
        "tape": " ".join([circle(t, HUBS[0], WIN_Y, 7.4, cw=True), circle(t, HUBS[1], WIN_Y, 5.4, cw=True)]),
        "hubs": " ".join(circle(t, x, 46.5 if small else WIN_Y, hub_r, cw=True)
                         for x in ((35.0, 65.0) if small else HUBS)),
        "hub_holes": " ".join(circle(t, x, WIN_Y, 2.5, cw=True) for x in HUBS),
        "teeth": " ".join(tooth(t, x, WIN_Y, a, 1.5, 2.7, 1.0) for x in HUBS for a in range(0, 360, 60)),
        "trap": poly(t, trap_pts()),
        "trap_holes": " ".join([circle(t, 41.0, 69.5, 1.6, cw=True), circle(t, 59.0, 69.5, 1.6, cw=True),
                                rrect(t, 30.0, 67.5, 34.0, 71.5, 1.0), rrect(t, 66.0, 67.5, 70.0, 71.5, 1.0),
                                rrect(t, 47.0, 66.5, 53.0, 69.0, 0.8)]),
        "screws": " ".join(circle(t, x, y, 1.15, cw=True)
                           for x, y in [(14.2, 28.2), (85.8, 28.2), (14.2, 70.8), (85.8, 70.8), (50.0, 60.4)]),
    }
    if label == "strip":
        k["label"] = rrect(t, 15.0, 28.0, 85.0, 36.0, 2.0)
        k["wave"] = wave(t, 19.0, 81.0, 32.0, 2.6, "bars")
    else:
        k["label"] = rrect(t, 15.0, 28.0, 85.0, 59.0, 2.6)
        k["wave"] = wave(t, 20.0, 80.0, 33.4, 4.0, "bars")
    if small:
        k["label"] = None      # too faint a step from the shell to read at 16 px
    return k


def draw_cassette(s, frame, k, c, small):
    vec(s, frame, "shell", k["shell"], fill=c["shell"])
    vec(s, frame, "trapezoid", k["trap"], fill=c["edge"])
    if k["label"]:
        vec(s, frame, "label", k["label"], fill=c["label"])
    vec(s, frame, "window", k["window"], fill=c["window"])
    if not small:
        vec(s, frame, "wave", k["wave"], fill=c["wave"])
        # The tape wound round each hub, clipped to the window.
        vec(s, frame, "tape", k["tape"], fill=c["tape"])
        vec(s, frame, "screws", k["screws"], fill=c["edge"])
        vec(s, frame, "trapezoid holes", k["trap_holes"], fill=c["window"])
    vec(s, frame, "hubs", k["hubs"], fill=c["hub"])
    if not small:
        vec(s, frame, "hub holes", k["hub_holes"], fill=c["window"])
        vec(s, frame, "teeth", k["teeth"], fill=c["hub"])


def lit_cassette(s, frame, t, c, label="peach", burst=None, glints=(), glow=True):
    """The cassette on the plate, a glow behind it, beams fanning out above it."""
    small = c.get("small")
    k = cassette(t, small=small, label=label)
    if glow:
        X, Y = t(50, 50)
        R = 48 * U * t.s
        s._node("ellipse", frame, "glow", X - R, Y - R, 2 * R, 2 * R, {"fills": [
            {"type": "radialGradient", "stops": [c["glow"] + "AA", c["glow"] + "33", c["glow"] + "00"]}]})
    if not small and burst:
        n, r0, r1, spread, a0, a1 = burst
        vec(s, frame, "rays", rays(t, 50, 50, n, r0, r1, spread, a0, a1), fill=c["rays"],
            opacity=c.get("rays_a", 1))
    draw_cassette(s, frame, k, c, small)
    if not small:
        for i, (gx, gy, gr) in enumerate(glints):
            vec(s, frame, f"glint {i + 1}", sparkle(t, gx, gy, gr), fill=c.get("glint", WHITE))


BEAMS = (7, 30, 85, 12, 200, 340)
GLINTS = [(14, 15, 4.2), (86, 16, 3.0), (84, 86, 2.6)]


def c1_beams(s, frame, t, c):
    lit_cassette(s, frame, t, c, burst=BEAMS, glints=GLINTS)


def c2_tilted(s, frame, t, c):
    lit_cassette(s, frame, t, c, burst=(12, 30, 85, 7, 0, 360), glints=[(14, 16, 4.0), (86, 84, 3.0)])


def c3_strip(s, frame, t, c):
    lit_cassette(s, frame, t, c, label="strip", glints=[(15, 14, 3.6)])


def cassette_glyph(t):
    """The cassette alone as ONE path, for one colour (small buttons): the shell, with the
    tape window cut out and the two hubs left standing in it (their holes cut), the
    trapezoid's outline cut as a thin line and its holes cut through."""
    k = cassette(t)
    inner = trap_pts(1.6)
    outer = trap_pts()
    return " ".join([
        rrect(t, CX0, CY0, CX1, CY1, 4.5),
        rrect(t, 26.0, 37.5, 74.0, 56.5, 9.5, ccw=True),
        " ".join(circle(t, x, WIN_Y, 6.2, cw=True) for x in HUBS),
        " ".join(circle(t, x, WIN_Y, 2.6) for x in HUBS),
        poly(t, list(reversed(outer))), poly(t, inner[:2] + [(inner[2][0], CY1), (inner[3][0], CY1)]),
        circle(t, 40.0, 69.5, 1.9), circle(t, 60.0, 69.5, 1.9),
    ])


def cassette_only(s, frame, t, c):
    """The cassette alone, flat, no glow: for small buttons and the toolbar."""
    draw_cassette(s, frame, cassette(t, small=c.get("small", False)), c, small=c.get("small", False))


def cassette_mono(s, frame, t, c):
    vec(s, frame, "cassette", cassette_glyph(t), fill=c["ink"])


CASSETTE = dict(plate=INK, glow=AMBER, rays=AMBER, rays_a=0.45, shell=AMBER, edge="#C46E1E",
                label=PEACH, window=INK, wave=INK, tape="#6A3E1E", hub=PEACH)
STRIP = dict(CASSETTE, label=INK2, wave=AMBER)
ON_PLATE_C = T(s=1.06)
SETS["cassette"] = [
    ("C1 beams", c1_beams, ON_PLATE_C, CASSETTE),
    ("C2 tilted", c2_tilted, T(-12, s=0.98), CASSETTE),
    ("C3 strip", c3_strip, T(s=1.12), STRIP),
    ("Cassette", cassette_only, T(s=1.2), dict(CASSETTE, plate=None)),
    ("Cassette mono", cassette_mono, T(s=1.2), dict(plate=None, ink=AMBER)),
]
# The cart that shipped from D-094 to 0.3.0 (R2 "beams"), kept to compare.
SETS["cart"] = [
    ("leadstinger", beams, ON_PLATE, R2),
    ("cart", plain_cart, GLYPH_FIT, dict(R2, plate=None)),
    ("cart-mono", plain_glyph, GLYPH_FIT, dict(plate=None, ink="#000000")),
]
# What ships: draft C1 (D-257). The Linux icon (full plate), the macOS icon
# (Apple's 824 px plate on 1024), each with its 16/32 px twin, and the cassette alone in
# colour and in one colour; scripts/icons.sh exports them.
MAC = (PX - 200) / PX          # Apple's plate, 824 of 1024 px
SETS["ship"] = [
    ("leadstinger", c1_beams, ON_PLATE_C, CASSETTE),
    ("macos", c1_beams, T(s=1.06 * MAC, dx=50 * MAC - 50, dy=50 * MAC - 50), dict(CASSETTE, inset=100)),
    ("cassette", cassette_only, T(s=1.2), dict(CASSETTE, plate=None)),
    ("cassette-mono", cassette_mono, T(s=1.2), dict(plate=None, ink="#000000")),
]


def script():
    s = Script()
    for i, (name, draw, t, c, *small) in enumerate(SETS[SET]):
        fr = s.frame(None, name, i * (PX + 96), 0, PX, PX)
        draw(s, plate(s, fr, c), t, c)
        # The 16/32 px twin: the same pose a little bigger, details dropped.
        sc = dict(small[0] if small else c, small=True)
        st = t if (t.s != 1 or SET in ("power", "ship", "cassette")) else T(math.degrees(t.a), 50, SMALL.cy, t.dx, t.dy, SMALL.s)
        fr = s.frame(None, name + " small", i * (PX + 96), PX + 96, PX, PX)
        draw(s, plate(s, fr, sc), st, sc)
    return s


if __name__ == "__main__":
    script().save(OUT)
