#!/usr/bin/env python3
"""Cut a starter pack's clips from your own files, and write the manifest the app reads.

Run it with `python3 -I scripts/clip-pack.py [CSV] [--media-root DIR] [--theme panel]
[--out DIR]` from the repo root. It reads starter/clips.csv by default and writes
starter/clips/<set>/<title>.flac plus starter/clips/manifest.json. It needs ffmpeg and ffprobe
on PATH.

CSV columns (a header row is required; extra columns are ignored):
  source_file  your video or audio file. A relative path is looked up under --media-root,
               then in the CSV's folder; ~ is expanded. Leave it (and episode)
               blank for a shot you haven't found yet: the row is skipped.
  start, end   seconds (83.5) or [HH:]MM:SS[.mmm]. Leave both blank to use the whole file
               (e.g. a clip quipclipper already cut).
  title        the tile title, and the clip's file name.
  set          the set (board) the tile goes in. Sets keep the order they first appear.
  hotkey       one of 1-0, a-z, ; , . /  (blank: the app picks a free key).
  color        red, orange, yellow, lime, green, mint, teal, cyan, blue, indigo, purple,
               pink, rose, crimson, brown or gray (blank: blue).
  mode         toggle, restart, hold or pause, optionally with "+loop" (hold+loop); plain
               "loop" means toggle+loop.
  fade_out     seconds the tile fades when it is stopped (blank: 0). Stored on the tile;
               the file only gets short anti-click fades.
Optional columns:
  episode      instead of source_file: an episode code such as S06E11. The clipper finds
               the one video under --media-root whose name contains it (S01E01 also
               matches a combined S01E01E02 file).
  stream       which audio stream to take, by a:N index as `quipclipper tracks` or ffprobe
               lists it (default 0).
  volume       the tile's volume, 0-1 (default 1).

Every clip is downmixed to stereo at 48 kHz, gets 10 ms / 25 ms fades, and is brought to
about -16 LUFS by a linear gain that never pushes the true peak above -1 dBTP. Running it
again only redoes clips whose row or source changed, and removes clips it made for rows
that are gone.
"""

import argparse
import csv
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

STAMP_VERSION = 1
TARGET_LUFS = -16.0
PEAK_CEILING = -1.0  # dBTP
FADE_IN = 0.010
FADE_OUT = 0.025
KEYS = "1234567890qwertyuiopasdfghjkl;zxcvbnm,./"
COLORS = {
    "red", "orange", "yellow", "lime", "green", "mint", "teal", "cyan",
    "blue", "indigo", "purple", "pink", "rose", "crimson", "brown", "gray",
}
MODES = {"toggle", "restart", "hold", "pause"}
REQUIRED = ["source_file", "start", "end", "title", "set", "hotkey", "color", "mode", "fade_out"]


class RowError(Exception):
    pass


def parse_time(text):
    """Seconds from "83.5", "1:23.5" or "0:01:23.5"; None for blank."""
    text = text.strip()
    if not text:
        return None
    parts = text.split(":")
    if len(parts) > 3:
        raise RowError(f"bad time {text!r}")
    try:
        secs = 0.0
        for p in parts:
            secs = secs * 60 + float(p)
    except ValueError:
        raise RowError(f"bad time {text!r}") from None
    if secs < 0:
        raise RowError(f"bad time {text!r}")
    return secs


def safe_name(text):
    """A file or folder name that works on macOS and Linux."""
    name = re.sub(r'[/\\:*?"<>|\x00-\x1f]', "-", text).strip().strip(".")
    return name or "Untitled"


def parse_mode(text):
    m = text.strip().lower() or "toggle"
    if m == "loop":
        return "toggle", True
    base, _, extra = m.partition("+")
    if base not in MODES or extra not in ("", "loop"):
        raise RowError(f"mode {text!r}: use toggle, restart, hold or pause, optionally +loop")
    return base, extra == "loop"


VIDEO_EXTS = {".mkv", ".mp4", ".m4v", ".avi", ".mov", ".webm", ".ts"}
_videos = None


def find_episode(code, media_root):
    """The one video file under media_root whose name contains the episode code."""
    global _videos
    if media_root is None:
        raise RowError(f"episode {code} needs --media-root")
    if _videos is None:
        _videos = sorted(
            p for p in media_root.rglob("*")
            if p.suffix.lower() in VIDEO_EXTS and not p.name.startswith(".") and p.is_file()  # not macOS ._ files
        )
    rx = re.compile(re.escape(code) + r"(?!\d)", re.I)
    hits = [p for p in _videos if rx.search(p.name)]
    if len(hits) != 1:
        raise RowError(f"episode {code}: {len(hits)} matching files under {media_root}")
    return hits[0]


def parse_row(row, csv_dir, media_root=None):
    title = row["title"].strip()
    set_name = row["set"].strip()
    if not title or not set_name:
        raise RowError("title and set are required")
    src = row["source_file"].strip()
    episode = (row.get("episode") or "").strip()
    source = None
    if src:
        p = Path(os.path.expanduser(src))
        if not p.is_absolute():
            p = next((d / p for d in (media_root, csv_dir) if d is not None and (d / p).exists()), csv_dir / p)
        source = p
    elif episode:
        source = find_episode(episode, media_root)
    start, end = parse_time(row["start"]), parse_time(row["end"])
    if end is not None and end <= (start or 0.0):
        raise RowError("end must come after start")
    key = row["hotkey"].strip().lower()
    if key and key not in KEYS:
        raise RowError(f"hotkey {row['hotkey']!r}: use one of {KEYS}")
    color = row["color"].strip().lower() or "blue"
    if color not in COLORS:
        raise RowError(f"color {row['color']!r}: use one of {', '.join(sorted(COLORS))}")
    mode, loop = parse_mode(row["mode"])
    try:
        fade_out = float(row["fade_out"].strip() or 0)
        volume = float((row.get("volume") or "").strip() or 1)
        stream = int((row.get("stream") or "").strip() or 0)
    except ValueError as e:
        raise RowError(str(e)) from None
    return {
        "source": source, "start": start, "end": end, "title": title, "set": set_name,
        "hotkey": key or None, "color": color, "mode": mode, "loop": loop,
        "fade_out": max(fade_out, 0.0), "volume": min(max(volume, 0.0), 1.0), "stream": stream,
    }


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        tail = "\n".join(r.stderr.strip().splitlines()[-4:])
        raise RowError(f"{Path(cmd[0]).name} failed: {tail}")
    return r


def duration(path):
    r = run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)])
    return float(r.stdout.strip())


def measure(path, dur):
    """(integrated LUFS, true peak dBTP). Short clips are looped past 4 s, because
    integrated loudness needs at least a few 400 ms gating blocks."""
    loops = max(0, int(4.0 / max(dur, 0.05)))
    r = run([
        "ffmpeg", "-hide_banner", "-nostats", "-stream_loop", str(loops), "-i", str(path),
        "-af", f"loudnorm=I={TARGET_LUFS}:TP={PEAK_CEILING}:print_format=json", "-f", "null", "-",
    ])
    blob = r.stderr[r.stderr.rfind("{"):r.stderr.rfind("}") + 1]
    stats = json.loads(blob)
    return float(stats["input_i"]), float(stats["input_tp"])


def cut(clip, out):
    """Cut, downmix, measure, gain and fade one clip into `out` (FLAC)."""
    with tempfile.TemporaryDirectory(prefix="clip-pack-") as tmp:
        raw = Path(tmp) / "raw.wav"
        cmd = ["ffmpeg", "-hide_banner", "-nostdin", "-y", "-v", "error"]
        if clip["start"]:
            cmd += ["-ss", f"{clip['start']:.3f}"]
        if clip["end"] is not None:
            cmd += ["-to", f"{clip['end']:.3f}"]
        cmd += [
            "-i", str(clip["source"]), "-map", f"0:a:{clip['stream']}", "-vn", "-sn", "-dn",
            "-ac", "2", "-ar", "48000", "-c:a", "pcm_f32le", str(raw),
        ]
        run(cmd)
        dur = duration(raw)
        if dur < 0.05:
            raise RowError("the clip is empty (check start/end and the stream)")
        lufs, peak = measure(raw, dur)
        if lufs == float("-inf"):
            raise RowError("the clip is silent")
        gain = min(TARGET_LUFS - lufs, PEAK_CEILING - peak)
        fade_out = min(FADE_OUT, dur / 4)
        af = (
            f"volume={gain:.2f}dB,afade=t=in:d={min(FADE_IN, dur / 4):.3f},"
            f"afade=t=out:st={dur - fade_out:.4f}:d={fade_out:.4f}"
        )
        out.parent.mkdir(parents=True, exist_ok=True)
        part = out.with_name(out.name + ".part")
        run([
            "ffmpeg", "-hide_banner", "-nostdin", "-y", "-v", "error", "-i", str(raw), "-af", af,
            "-c:a", "flac", "-sample_fmt", "s16", "-f", "flac", str(part),
        ])
        part.replace(out)
        return lufs, peak, gain


def stamp(clip):
    st = clip["source"].stat()
    key = json.dumps([
        STAMP_VERSION, str(clip["source"].resolve()), st.st_size, int(st.st_mtime),
        clip["start"], clip["end"], clip["stream"], TARGET_LUFS, PEAK_CEILING,
    ])
    return hashlib.sha256(key.encode()).hexdigest()[:16]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    root = Path(__file__).resolve().parent.parent
    ap.add_argument("csv", nargs="?", default=root / "starter" / "clips.csv", type=Path)
    ap.add_argument("--media-root", type=Path, help="folder holding your episodes (for source_file paths and the episode column)")
    ap.add_argument("--out", default=root / "starter" / "clips", type=Path, help="output folder (default starter/clips)")
    ap.add_argument("--theme", choices=["console", "panel", "padd"], help="the look the app opens in with this pack")
    ap.add_argument("--force", action="store_true", help="recut every clip")
    args = ap.parse_args()

    for tool in ("ffmpeg", "ffprobe"):
        if not shutil.which(tool):
            sys.exit(f"clip-pack: {tool} isn't on PATH")
    if not args.csv.exists():
        sys.exit(f"clip-pack: {args.csv} doesn't exist (copy starter/clips.example.csv to start)")

    out_dir = args.out
    stamps_path = out_dir / ".stamps.json"
    try:
        old_stamps = json.loads(stamps_path.read_text())
    except (OSError, ValueError):
        old_stamps = {}
    new_stamps = {}
    sets = {}  # name -> list of manifest tiles, in CSV order
    used = set()
    failed = skipped = made = kept = 0

    with args.csv.open(newline="", encoding="utf-8-sig") as f:
        reader = csv.DictReader(f)
        missing = [c for c in REQUIRED if c not in (reader.fieldnames or [])]
        if missing:
            sys.exit(f"clip-pack: {args.csv} lacks the column(s) {', '.join(missing)}")
        for n, row in enumerate(reader, start=2):
            try:
                clip = parse_row(row, args.csv.parent, args.media_root)
            except RowError as e:
                print(f"line {n}: {e}", file=sys.stderr)
                failed += 1
                continue
            if clip["source"] is None:
                skipped += 1
                continue
            rel = f"{safe_name(clip['set'])}/{safe_name(clip['title'])}.flac"
            if rel.lower() in used:
                print(f"line {n}: {rel} is already used by an earlier row", file=sys.stderr)
                failed += 1
                continue
            used.add(rel.lower())
            out = out_dir / rel
            try:
                if not clip["source"].exists():
                    raise RowError(f"{clip['source']} doesn't exist")
                s = stamp(clip)
                if not args.force and old_stamps.get(rel) == s and out.exists():
                    kept += 1
                else:
                    lufs, peak, gain = cut(clip, out)
                    made += 1
                    print(f"{rel}: {lufs:.1f} LUFS, peak {peak:.1f} dBTP, gain {gain:+.1f} dB")
                new_stamps[rel] = s
            except RowError as e:
                print(f"line {n} ({clip['title']}): {e}", file=sys.stderr)
                failed += 1
                if out.exists() and rel in old_stamps:
                    new_stamps[rel] = old_stamps[rel]  # keep the last good cut
                else:
                    continue
            tile = {k: clip[k] for k in ("title", "hotkey", "color", "mode", "loop", "fade_out", "volume")}
            sets.setdefault(clip["set"], []).append({"file": rel, **tile})

    # Remove clips this script made for rows that are gone (never anything else).
    removed = 0
    for rel in old_stamps.keys() - new_stamps.keys():
        p = out_dir / rel
        if p.is_file():
            p.unlink()
            removed += 1
            try:
                p.parent.rmdir()
            except OSError:
                pass

    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = {
        "format": "leadstinger-starter",
        "version": 1,
        "sets": [{"name": name, "tiles": tiles} for name, tiles in sets.items()],
    }
    if args.theme:
        manifest["theme"] = args.theme
    for path, data in ((out_dir / "manifest.json", manifest), (stamps_path, new_stamps)):
        tmp = path.with_name(path.name + ".part")
        tmp.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        tmp.replace(path)

    total = sum(len(t) for t in sets.values())
    print(
        f"{total} clips in {len(sets)} sets: {made} cut, {kept} unchanged, {removed} removed, "
        f"{skipped} not found yet, {failed} with errors. Manifest: {out_dir / 'manifest.json'}"
    )
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
