#!/usr/bin/env bash
# Try Apple Speech transcription for real (macOS, D-286 to D-289), without touching your
# library: a scratch library, dist/LeadStinger.app opened in the background, and its
# leadstinger-cli talking to it. Build the bundle first (scripts/bundle-macos.sh).
#
#   scripts/apple-speech-check.sh            report the permission, the language and
#                                            whether it's on the device; never prompts
#   scripts/apple-speech-check.sh --allow    if permission hasn't been asked, show the
#                                            system's prompt (it comes up in front), wait
#                                            for your answer, then transcribe
#   scripts/apple-speech-check.sh --allow FILE   transcribe FILE instead of the spoken clip
#
# Without permission it transcribes nothing: the engine refuses and says why. The
# spoken clip is macOS `say`; the script ends by searching the tile for a heard word.
set -euo pipefail
cd "$(dirname "$0")/.."
app=$PWD/dist/LeadStinger.app
cli=$app/Contents/MacOS/leadstinger-cli
allow=0 audio=
for a in "$@"; do
  case $a in
    --allow) allow=1 ;;
    -*) echo "unknown option $a" >&2; exit 2 ;;
    *) audio=$a ;;
  esac
done
[ -x "$cli" ] || { echo "no $app: run scripts/bundle-macos.sh first" >&2; exit 1; }

data=$(mktemp -d /tmp/ls-speech.XXXXXX)
ls_() { "$cli" --data-dir "$data" "$@"; }
trap 'ls_ app.quit >/dev/null 2>&1 || true' EXIT

if [ -z "$audio" ]; then
  audio=$data/spoken.aiff
  say -o "$audio" "Welcome to the spring concert. Please switch off your phones, and enjoy the music."
fi

open -g -n "$app" --env LEADSTINGER_DATA_DIR="$data" --env LEADSTINGER_SILENT=1 --env LEADSTINGER_BACKGROUND=1
for _ in $(seq 300); do ls_ status >/dev/null 2>&1 && break; sleep 0.2; done
ls_ status >/dev/null || { echo "the app didn't open its control socket" >&2; exit 1; }

echo "== Apple Speech, as the app sees it (asking never prompts)"
ls_ --json transcribe.engines

if [ $allow = 1 ]; then
  echo "== Asking for permission (only if it hasn't been asked)"
  ls_ --json transcribe.allowSpeech
  printf 'Answer the system prompt if one came up, then press Return. '
  read -r _
  ls_ --json transcribe.engines
fi

echo "== Adding $(basename "$audio") and transcribing it with Apple Speech"
ls_ --json tile.add "$audio" >/dev/null
title=$(basename "${audio%.*}")
for _ in $(seq 100); do ls_ --json tile.get "$title" >/dev/null 2>&1 && break; sleep 0.2; done
sleep 1
ls_ tile.transcribe "$title" --engine apple || true
ls_ --json tile.subtitles "$title"
word=$(ls_ --json tile.subtitles "$title" | python3 -c 'import json,sys; c=json.load(sys.stdin)["cues"]; w=[x["text"] for l in c for x in l.get("words",[]) if len(x["text"])>4]; print(w[0].strip(".,!?") if w else "")')
if [ -n "$word" ]; then
  echo "== tile.search $word"
  ls_ --json tile.search "$word"
fi
echo "(scratch library: $data)"
