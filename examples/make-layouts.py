#!/usr/bin/env python3
"""Write the control-surface layouts in this folder from one design.

    python3 examples/make-layouts.py

It writes companion/leadstinger.companionconfig, touchosc/leadstinger.tosc and
open-stage-control/leadstinger.json. Every layout uses only the shipped /ls/ addresses
(docs/CLI.md, "OSC"); a test in src/osc.rs checks that. The output is the same on every
run, so a change to this script shows up as a reviewable diff.

Written for LeadStinger from the apps' file formats; no one else's layout was used.
"""

import json
import pathlib
import uuid
import zlib

HERE = pathlib.Path(__file__).resolve().parent

# Tile colours as the Console theme draws them (src/theme.rs, tile_fill).
COLORS = {
    "blue": 0x5684D6, "cyan": 0x5EAAD6, "teal": 0x50BEA8, "green": 0x68BA62,
    "lime": 0xA8C44C, "yellow": 0xDEC448, "orange": 0xE6983E, "red": 0xDE6848,
    "crimson": 0xD6526E, "pink": 0xC456A8, "purple": 0x9662CE, "indigo": 0x6E68D6,
    "gray": 0x8296AA, "brown": 0xB28C6E, "mint": 0x78AA8C, "rose": 0xBE7896,
}
WHITE, BLACK, DARK = 0xFFFFFF, 0x000000, 0x262626
GO_GREEN, STOP_RED, TALK_ORANGE, PAUSED = 0x2E9E4F, 0xC0392B, 0xE6983E, 0x7F6A1F


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data if isinstance(data, bytes) else data.encode())
    print("wrote", path.relative_to(HERE.parent))


# ── Bitfocus Companion ─────────────────────────────────────────────────────────
# A page export (version 12, the plain button shape Companion 4.3 writes; 5.x converts
# it on import) for the generic OSC module 3.0 or later, whose value feedbacks feed the
# keys' local variables. 8 x 4 keys (Stream Deck XL): 24 tiles, then the controls.

CONN = "leadstinger"


def opt(v):
    return {"isExpression": False, "value": v}


class Ids:
    def __init__(self):
        self.n = 0

    def __call__(self, kind):
        self.n += 1
        return f"ls-{kind}-{self.n}"


def companion():
    new_id = Ids()

    def action(definition, **options):
        return {"type": "action", "id": new_id("a"), "definitionId": definition, "connectionId": CONN,
                "upgradeIndex": 3, "options": {k: opt(v) for k, v in options.items()}}

    def feedback(definition, style, **options):
        return {"type": "feedback", "id": new_id("f"), "definitionId": definition, "connectionId": CONN,
                "upgradeIndex": 3, "options": {k: opt(v) for k, v in options.items()},
                "isInverted": opt(False), "style": style}

    def value(name, path):
        return {"type": "feedback", "id": new_id("v"), "definitionId": "osc_feedback_value", "connectionId": CONN,
                "upgradeIndex": 3, "variableName": name, "options": {"path": opt(path), "index": opt(0)}}

    def key(text, bg, down=(), up=(), feedbacks=(), local=(), size="auto", color=WHITE):
        return {
            "type": "button",
            "options": {"stepProgression": "auto", "rotaryActions": False},
            "style": {"text": text, "textExpression": False, "size": size, "alignment": "center:center",
                      "pngalignment": "center:center", "color": color, "bgcolor": bg,
                      "show_topbar": False, "png64": None},
            "localVariables": list(local),
            "feedbacks": list(feedbacks),
            "steps": {"0": {"options": {"runWhileHeld": []},
                            "action_sets": {"down": list(down), "up": list(up)}}},
        }

    def tile(n):
        base = f"/ls/tile/{n}"
        colours = [feedback("osc_feedback_string", {"bgcolor": rgb, "color": WHITE},
                            path=f"{base}/color", arguments=name, comparison="equal")
                   for name, rgb in COLORS.items()]
        return key(
            "$(local:title)\\n$(local:time)", DARK,
            down=[action("send_int", path=f"{base}/trigger", int=1)],
            up=[action("send_int", path=f"{base}/trigger", int=0)],
            local=[value("title", f"{base}/title"), value("time", f"{base}/time")],
            feedbacks=colours + [
                # Lit while it plays, amber while paused.
                feedback("osc_feedback_float", {"bgcolor": WHITE, "color": BLACK},
                         path=f"{base}/state", arguments=1, comparison="equal"),
                feedback("osc_feedback_float", {"bgcolor": PAUSED, "color": WHITE},
                         path=f"{base}/state", arguments=2, comparison="equal"),
            ],
            size=14,
        )

    controls = {}
    for n in range(1, 25):
        row, col = divmod(n - 1, 8)
        controls.setdefault(str(row), {})[str(col)] = tile(n)
    controls["3"] = {
        "0": key("GO\\n$(local:next)", GO_GREEN, down=[action("send_blank", path="/ls/go")],
                 local=[value("next", "/ls/cue/next/title")], size=14),
        "1": key("STOP\\nALL", STOP_RED, down=[action("send_blank", path="/ls/stopall")], size=18),
        "2": key("TALK", DARK, down=[action("send_int", path="/ls/duck", int=1)],
                 up=[action("send_int", path="/ls/duck", int=0)],
                 feedbacks=[feedback("osc_feedback_float", {"bgcolor": TALK_ORANGE, "color": BLACK},
                                     path="/ls/duck", arguments=1, comparison="equal")], size=18),
        "3": key("VOL\\n−", DARK, down=[action("send_string", path="/ls/cmd", string="master.down")], size=18),
        "4": key("VOL\\n+", DARK, down=[action("send_string", path="/ls/cmd", string="master.up")], size=18),
        "5": key("$(local:title)", DARK, local=[value("title", "/ls/now/title")], size=14),
        "6": key("$(local:time)", DARK, local=[value("time", "/ls/now/time")], size=30),
        "7": key("SYNC", DARK, down=[action("send_blank", path="/ls/resync")], size=14),
    }
    page = {
        "version": 12,
        "type": "page",
        "companionBuild": "LeadStinger examples/make-layouts.py",
        "oldPageNumber": 1,
        "connectionCollections": [],
        "instances": {
            CONN: {
                "label": CONN,
                "moduleId": "generic-osc",
                "lastUpgradeIndex": 3,
                "enabled": True,
                "isFirstInit": False,
                "sortOrder": 0,
                "config": {"host": "127.0.0.1", "targetPort": 9000, "protocol": "udp",
                           "listen": True, "feedbackPort": "9001"},
            }
        },
        "page": {
            "name": "LeadStinger",
            "gridSize": {"minColumn": 0, "maxColumn": 7, "minRow": 0, "maxRow": 3},
            "controls": controls,
        },
    }
    return json.dumps(page, ensure_ascii=False, separators=(",", ":")) + "\n"


# ── TouchOSC ───────────────────────────────────────────────────────────────────
# A zlib-compressed lexml document. 1024 x 768 (a tablet held sideways): 16 tile
# buttons, each with its title and countdown on top, and a bottom row of controls.
# Buttons send on touch (1 on press, 0 on release) and show the tile's state through
# their x value, so feedback never sends anything back. The root script colours the
# tiles from /ls/tile/N/rgb and asks for everything when the layout starts.

ROOT_SCRIPT = """\
-- LeadStinger: tile colours from /ls/tile/N/rgb (#RRGGBB), and a resync on start.
function init()
  sendOSC('/ls/resync')
end

function onReceiveOSC(message, connections)
  local n = string.match(message[1], '^/ls/tile/(%d+)/rgb$')
  if n then
    local tile = root:findByName('tile' .. n, true)
    local hex = message[2][1] and message[2][1].value or ''
    if tile and #hex == 7 then
      tile.color = Color.fromHexString(string.sub(hex, 2) .. 'FF')
    end
    return true
  end
  return false
end
"""


def esc(s):
    return s.replace("]]>", "]]]]><![CDATA[>")


class Tosc:
    def __init__(self):
        self.count = 0

    def node_id(self, name):
        self.count += 1
        return str(uuid.uuid5(uuid.NAMESPACE_URL, f"leadstinger-touchosc/{name}/{self.count}"))

    @staticmethod
    def prop(kind, key, value):
        if kind == "r":
            x, y, w, h = value
            v = f"<x>{x}</x><y>{y}</y><w>{w}</w><h>{h}</h>"
        elif kind == "c":
            r, g, b, a = value
            v = f"<r>{r:.3f}</r><g>{g:.3f}</g><b>{b:.3f}</b><a>{a}</a>"
        elif kind == "s":
            v = f"<![CDATA[{esc(value)}]]>"
        else:
            v = str(int(value) if kind in "bi" else value)
        return f"<property type='{kind}'><key><![CDATA[{key}]]></key><value>{v}</value></property>"

    @staticmethod
    def value(key, default):
        return (f"<value><key><![CDATA[{key}]]></key><locked>0</locked><lockedDefaultCurrent>0</lockedDefaultCurrent>"
                f"<default><![CDATA[{default}]]></default><defaultPull>0</defaultPull></value>")

    @staticmethod
    def osc(path, send=False, receive=False, trigger=None, args=()):
        """args: (value key, conversion) pairs; trigger: (value key, ANY|RISE|FALL)."""
        triggers = (f"<triggers><trigger><var><![CDATA[{trigger[0]}]]></var><condition>{trigger[1]}</condition></trigger></triggers>"
                    if trigger else "")
        parts = "".join(f"<partial><type>VALUE</type><conversion>{conv}</conversion><value><![CDATA[{key}]]></value>"
                        f"<scaleMin>0</scaleMin><scaleMax>1</scaleMax></partial>" for key, conv in args)
        return (f"<osc><enabled>1</enabled><send>{int(send)}</send><receive>{int(receive)}</receive><feedback>0</feedback>"
                f"<noDuplicates>0</noDuplicates><connections>1111111111</connections>{triggers}"
                f"<path><partial><type>CONSTANT</type><conversion>STRING</conversion><value><![CDATA[{path}]]></value>"
                f"<scaleMin>0</scaleMin><scaleMax>1</scaleMax></partial></path>"
                f"<arguments>{parts}</arguments></osc>")

    def node(self, kind, name, props, values, messages=(), children=()):
        props = sorted(props + [("s", "name", name)], key=lambda p: p[1])
        out = f"<node ID='{self.node_id(name)}' type='{kind}'><properties>"
        out += "".join(self.prop(*p) for p in props) + "</properties><values>"
        out += "".join(self.value(*v) for v in values) + "</values>"
        if messages:
            out += "<messages>" + "".join(messages) + "</messages>"
        if children:
            out += "<children>" + "".join(children) + "</children>"
        return out + "</node>"

    def button(self, name, frame, rgb, press_path=None, rise_only=False, state_path=None, args=True):
        msgs = []
        if press_path:
            msgs.append(self.osc(press_path, send=True, trigger=("touch", "RISE" if rise_only else "ANY"),
                                 args=[("touch", "INTEGER")] if args else []))
        if state_path:
            msgs.append(self.osc(state_path, receive=True, args=[("x", "FLOAT")]))
        return self.node("BUTTON", name, [("r", "frame", frame), ("c", "color", rgb), ("i", "buttonType", 0),
                                          ("b", "press", 1), ("b", "release", 1), ("f", "cornerRadius", 6)],
                         [("x", 0), ("touch", "false")], msgs)

    def label(self, name, frame, path, size, text="", align_v=2):
        return self.node("LABEL", name, [("r", "frame", frame), ("b", "interactive", 0), ("b", "background", 0),
                                         ("i", "textSize", size), ("i", "textAlignH", 2), ("i", "textAlignV", align_v),
                                         ("c", "textColor", (1, 1, 1, 1))],
                         [("text", text), ("touch", "false")],
                         [self.osc(path, receive=True, args=[("text", "STRING")])] if path else [])

    def fader(self, name, frame, path):
        return self.node("FADER", name, [("r", "frame", frame), ("c", "color", rgb(0x5684D6)), ("i", "orientation", 1)],
                         [("x", 0), ("touch", "false")],
                         [self.osc(path, send=True, trigger=("x", "ANY"), args=[("x", "FLOAT")]),
                          self.osc(path, receive=True, args=[("x", "FLOAT")])])


def rgb(n, a=1):
    return ((n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, a)


def touchosc():
    t = Tosc()
    kids = []
    w, h, gap, top = 244, 140, 12, 12
    for n in range(1, 17):
        row, col = divmod(n - 1, 4)
        x, y = gap + col * (w + gap), top + row * (h + gap)
        base = f"/ls/tile/{n}"
        kids.append(t.button(f"tile{n}", (x, y, w, h), rgb(0x8296AA), press_path=f"{base}/trigger", state_path=f"{base}/state"))
        kids.append(t.label(f"title{n}", (x + 8, y + 8, w - 16, 60), f"{base}/title", 20, f"Tile {n}"))
        kids.append(t.label(f"time{n}", (x + 8, y + h - 52, w - 16, 44), f"{base}/time", 28))
    y = top + 4 * (h + gap)
    bh = 768 - y - gap
    kids.append(t.button("go", (gap, y, 180, bh), rgb(GO_GREEN), press_path="/ls/go", rise_only=True, args=False))
    kids.append(t.label("go-label", (gap, y + 8, 180, 40), None, 30, "GO"))
    kids.append(t.label("next", (gap, y + bh - 44, 180, 36), "/ls/cue/next/title", 16))
    kids.append(t.button("stopall", (gap + 192, y, 140, bh), rgb(STOP_RED), press_path="/ls/stopall", rise_only=True, args=False))
    kids.append(t.label("stop-label", (gap + 192, y, 140, bh), None, 22, "STOP ALL"))
    kids.append(t.button("talk", (gap + 344, y, 140, bh), rgb(TALK_ORANGE), press_path="/ls/duck", state_path="/ls/duck"))
    kids.append(t.label("talk-label", (gap + 344, y, 140, bh), None, 22, "TALK"))
    kids.append(t.fader("master", (gap + 496, y, 160, bh), "/ls/master"))
    kids.append(t.label("now-title", (gap + 668, y + 4, 230, 40), "/ls/now/title", 18))
    kids.append(t.label("now-time", (gap + 668, y + 40, 230, bh - 44), "/ls/now/time", 54))
    kids.append(t.button("resync", (1024 - gap - 100, y, 100, bh), rgb(0x404040), press_path="/ls/resync", rise_only=True, args=False))
    kids.append(t.label("resync-label", (1024 - gap - 100, y, 100, bh), None, 16, "SYNC"))
    root = (f"<node ID='{t.node_id('root')}' type='GROUP'><includes></includes><properties>"
            + "".join(t.prop(*p) for p in sorted([("r", "frame", (0, 0, 1024, 768)), ("c", "color", rgb(0x1A1A1A)),
                                                    ("s", "name", "LeadStinger"), ("s", "script", ROOT_SCRIPT)],
                                                   key=lambda p: p[1]))
            + "</properties><values>" + t.value("touch", "false") + "</values>"
            + "<children>" + "".join(kids) + "</children></node>")
    xml = "<?xml version='1.0' encoding='UTF-8'?>\n<lexml version='6'>" + root + "</lexml>\n"
    return zlib.compress(xml.encode("utf-8"), 9)


# ── Open Stage Control ─────────────────────────────────────────────────────────
# A session for Open Stage Control 1.x, started with
#   open-stage-control --send 127.0.0.1:9000 --osc-port 9001 --load leadstinger.json
# Tile buttons take their label, colour and fill from the feedback addresses.

def open_stage_control():
    tiles = []
    for n in range(1, 17):
        base = f"/ls/tile/{n}"
        tiles.append({
            "type": "button", "id": f"tile{n}", "mode": "push", "on": 1, "off": 0,
            "address": f"{base}/trigger", "typeTags": "i",
            "label": f"#{{OSC{{{base}/title}} || 'Tile {n}'}}",
            "colorWidget": f"JS{{ return OSC{{{base}/rgb}} || '#8296AA' }}",
            "alphaFillOff": f"JS{{ var s = OSC{{{base}/state}}; return s == 1 ? 0.9 : s == 2 ? 0.45 : 0.15 }}",
            "wrap": True, "expand": True,
        })
        tiles.append({"type": "text", "id": f"time{n}", "address": f"{base}/time", "default": "",
                      "height": 30})
    controls = [
        {"type": "button", "id": "go", "mode": "tap", "on": 1, "address": "/ls/go", "typeTags": "i",
         "label": "#{'GO ' + (OSC{/ls/cue/next/title} || '')}", "colorWidget": "#2E9E4F"},
        {"type": "button", "id": "stopall", "mode": "tap", "on": 0, "address": "/ls/stopall", "typeTags": "i",
         "label": "STOP ALL", "colorWidget": "#C0392B"},
        {"type": "button", "id": "talk", "mode": "push", "on": 1, "off": 0, "address": "/ls/duck", "typeTags": "i",
         "label": "TALK", "colorWidget": "#E6983E"},
        {"type": "fader", "id": "master", "address": "/ls/master", "range": {"min": 0, "max": 1},
         "horizontal": True, "colorWidget": "#5684D6", "expand": True},
        {"type": "text", "id": "now-title", "address": "/ls/now/title", "default": "Nothing playing"},
        {"type": "text", "id": "now-time", "address": "/ls/now/time", "default": "", "css": "font-size: 200%;", "width": 160},
        {"type": "button", "id": "resync", "mode": "tap", "on": None, "address": "/ls/resync", "label": "SYNC"},
    ]
    session = {
        "createdWith": "Open Stage Control",
        "version": "1.31.1",
        "type": "session",
        "content": {
            "type": "root",
            "layout": "vertical",
            "colorBg": "#1A1A1A",
            "widgets": [
                {"type": "panel", "id": "tiles", "layout": "grid", "gridTemplate": 4,
                 "expand": True, "widgets": [
                     {"type": "panel", "id": f"cell{n}", "layout": "vertical", "widgets": tiles[2 * n - 2: 2 * n]}
                     for n in range(1, 17)]},
                {"type": "panel", "id": "controls", "layout": "horizontal", "height": 120, "widgets": controls},
            ],
        },
    }
    return json.dumps(session, indent=1) + "\n"


if __name__ == "__main__":
    write(HERE / "companion" / "leadstinger.companionconfig", companion())
    write(HERE / "touchosc" / "leadstinger.tosc", touchosc())
    write(HERE / "open-stage-control" / "leadstinger.json", open_stage_control())
