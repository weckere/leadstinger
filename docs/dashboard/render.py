#!/usr/bin/env python3
"""Fill the project board template with one project's details.

Usage: render.py CONFIG.json [OUT.html]   (default: board.html next to this script)

CONFIG holds the values for the {{PLACEHOLDERS}} in template.html; see example.json.
Values are inserted as HTML, so escape any <, > or & in plain text.
"""
import json
import os
import re
import sys

here = os.path.dirname(os.path.abspath(__file__))
if len(sys.argv) < 2:
    sys.exit(__doc__)
config = json.load(open(sys.argv[1], encoding="utf-8"))
page = open(os.path.join(here, "template.html"), encoding="utf-8").read()
missing = sorted(set(re.findall(r"\{\{([A-Z_]+)\}\}", page)) - set(config))
if missing:
    sys.exit(f"render.py: {sys.argv[1]} has no value for {', '.join(missing)}")
for key, value in config.items():
    page = page.replace("{{" + key + "}}", value)
out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "board.html")
with open(out, "w", encoding="utf-8") as fh:
    fh.write(page)
print(f"render.py: wrote {out}")
