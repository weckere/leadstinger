#!/usr/bin/env python3
"""Drive the pointer of a wlroots compositor (sway) through zwlr_virtual_pointer_v1.

Usage: vpointer.py WIDTH HEIGHT STEP...
Steps: "move X Y", "glide X1 Y1 X2 Y2 N" (N moves, 30 ms apart), "down", "up",
"wait SECONDS". Positions are in the output layout's pixels.

The protocol bindings come from pywayland's scanner, generated as the package
`protocols` in the directory $VPOINTER_PROTOCOLS (see run.sh).
"""
import os
import sys
import time

sys.path.insert(0, os.environ["VPOINTER_PROTOCOLS"])

from pywayland.client import Display  # noqa: E402
from protocols.wayland import WlSeat  # noqa: E402
from protocols.wlr_virtual_pointer_unstable_v1 import ZwlrVirtualPointerManagerV1  # noqa: E402

BTN_LEFT = 0x110


def main():
    width, height = int(sys.argv[1]), int(sys.argv[2])
    steps = sys.argv[3:]
    display = Display()
    display.connect()
    registry = display.get_registry()
    found = {}
    registry.dispatcher["global"] = lambda _r, name, iface, version: found.setdefault(iface, name)
    display.roundtrip()
    seat = registry.bind(found["wl_seat"], WlSeat, 1)
    manager = registry.bind(found["zwlr_virtual_pointer_manager_v1"], ZwlrVirtualPointerManagerV1, 1)
    pointer = manager.create_virtual_pointer(seat)

    def now():
        return int(time.monotonic() * 1000) & 0xFFFFFFFF

    def move(x, y):
        pointer.motion_absolute(now(), int(x), int(y), width, height)
        pointer.frame()
        display.roundtrip()

    def button(pressed):
        pointer.button(now(), BTN_LEFT, 1 if pressed else 0)
        pointer.frame()
        display.roundtrip()

    for step in steps:
        word, *args = step.split()
        nums = [float(a) for a in args]
        if word == "move":
            move(*nums)
        elif word == "glide":
            x1, y1, x2, y2, n = nums
            for i in range(1, int(n) + 1):
                move(x1 + (x2 - x1) * i / n, y1 + (y2 - y1) * i / n)
                time.sleep(0.03)
        elif word == "down":
            button(True)
        elif word == "up":
            button(False)
        elif word == "wait":
            time.sleep(nums[0])
        else:
            sys.exit(f"unknown step: {step}")
        print(f"vpointer: {step}", flush=True)
    pointer.destroy()
    display.roundtrip()
    display.disconnect()


if __name__ == "__main__":
    main()
