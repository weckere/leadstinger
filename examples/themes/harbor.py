#!/usr/bin/env python3
"""Harbor: an example LeadStinger theme (t-m19-themes), made with FigCraft.

Run it to rebuild harbor.lstheme/tokens.json:

    python3 examples/themes/harbor.py

It needs figcraft-cli on the PATH. The theme only names the tokens it changes; every
other value (the tile colours, the high-contrast look, the fonts) comes from Console.
"""
import json, pathlib, subprocess, tempfile

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "harbor.lstheme"

# name: (Dark, Light)
COLORS = {
    "window": ("#0F1C26", "#D9D2C3"),
    "panel": ("#15242F", "#E6E0D3"),
    "panel2": ("#1A2B37", "#DDD6C8"),
    "bar": ("#0B151D", "#CFC7B6"),
    "group": ("#0D1820", "#D8D1C2"),
    "border": ("#06101A", "#9C9282"),
    "borderLight": ("#2C4252", "#ADA393"),
    "text": ("#DCE8EE", "#1A2128"),
    "textDim": ("#9BB3C1", "#3E4750"),
    "accent": ("#2FB3A6", "#1E7F76"),
    "accentDark": ("#1B6F67", "#1B6F67"),
    "key": ("#22394A", "#F2EEE6"),
    "keyHover": ("#2E4B5F", "#FAF7F1"),
    "keyBorder": ("#07121A", "#9C9282"),
    "selected": ("#1F4A5A", "#B9D6D0"),
    "counterBg": ("#04090D", "#04090D"),
    "counterText": ("#7FE3D0", "#7FE3D0"),
    "playhead": ("#FF6B5A", "#FF6B5A"),
    "meterGreen": ("#3CC8A0", "#1E8F6A"),
    "meterYellow": ("#E8C547", "#A88A10"),
    "meterRed": ("#F05A4A", "#C23A2E"),
    "meterOff": ("#071017", "#1C1E1C"),
    "stop": ("#7A2230", "#7A2230"),
    "playing": ("#1E5E50", "#1E5E50"),
    "solo": ("#E8C547", "#E8C547"),
    "focus": ("#8FD8FF", "#0D4F8A"),
    # Status colours (D-120): errors, the drop frame, trim handles, waveform marks and
    # the input level, darker in Light so they reach WCAG AA there too.
    "status/error": ("#FF8A7A", "#9A1F16"),
    "status/drop": ("#7FE3D0", "#7FE3D0"),
    "status/trim": ("#F2C14E", "#4A2E00"),
    "mark/marker": ("#F49AD8", "#5E1050"),
    "mark/vamp": ("#7FD8F0", "#0A3E4E"),
    "input/voice": ("#F0A040", "#9A4A00"),
    "input/level": ("#3CC8A0", "#1E7A50"),
}
FLOATS = {"radius/control": 5, "radius/window": 10, "radius/menu": 8}

steps = [{"command": "variable.createCollection", "params": {"name": "Harbor", "modes": ["Dark", "Light"]}}]
for name, (dark, light) in COLORS.items():
    steps.append({"command": "variable.create", "params": {"collection": "Harbor", "name": name, "type": "color", "values": {"Dark": dark, "Light": light}}})
for name, v in FLOATS.items():
    steps.append({"command": "variable.create", "params": {"collection": "Harbor", "name": name, "type": "float", "value": v}})
OUT.mkdir(exist_ok=True)
steps.append({"command": "variable.export", "params": {"collections": ["Harbor"], "format": "json", "path": str(OUT / "tokens.json")}})

with tempfile.TemporaryDirectory() as tmp:
    script = pathlib.Path(tmp) / "harbor.json"
    script.write_text(json.dumps(steps))
    subprocess.run(["figcraft-cli", "run", str(script), "--out", str(pathlib.Path(tmp) / "harbor.figcraft")], check=True, stdout=subprocess.DEVNULL)
(OUT / "theme.json").write_text(json.dumps({"name": "Harbor", "base": "console"}, indent=2) + "\n")
print(f"wrote {OUT}")
