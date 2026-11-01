# Recipes

Short how-tos for wiring LeadStinger into the rest of a show, a stream or a podcast. Each
one uses only what the app already has: system-wide keys, the command line, OSC and
outputs. Commands are `leadstinger-cli`; see [CLI.md](CLI.md) for every option.

- [A Stream Deck key](#a-stream-deck-key)
- [Bitfocus Companion, with countdowns](#bitfocus-companion-with-countdowns)
- [OBS: LeadStinger on its own track](#obs-leadstinger-on-its-own-track)
- [A virtual microphone](#a-virtual-microphone)
- [Event hooks with `watch`](#event-hooks-with-watch)
- [Vamp until cue](#vamp-until-cue)

Key binds on Hyprland and other Wayland compositors are in the README
([Hyprland and other Wayland compositors](../README.md#hyprland-and-other-wayland-compositors)),
and the Bitfocus Companion, TouchOSC and Open Stage Control layouts are in
[`examples/`](../examples/).

## A Stream Deck key

There are three ways, from simplest to richest.

**A system-wide key** (macOS, and Linux under X11). Bind a key combination nobody else
uses to the tile, then make the Stream Deck key press it:

```bash
leadstinger-cli bind.add key:Ctrl+Alt+Cmd+1 --tile "Air Horn"   # Ctrl+Alt+Super+1 on Linux
```

(or select the tile and click **Key…** on the inspector's Control tab). In the Stream Deck
app, drag **System ▸ Hotkey** onto a key and press the same combination. The key works
while another app has focus. Hold-to-play tiles follow the key: the Stream Deck sends the
press and the release.

**A command.** Every action is a `leadstinger-cli` command, so anything that can run a
program can fire a tile. On macOS, save this as `air-horn.command`, make it executable
(`chmod +x air-horn.command`) and point a **System ▸ Open** key at it:

```bash
#!/bin/sh
exec /Applications/LeadStinger.app/Contents/MacOS/leadstinger-cli trigger "Air Horn"
```

On Linux, Stream Deck apps such as StreamController and streamdeck-ui have a command
action: `leadstinger-cli trigger "Air Horn"`, `leadstinger-cli go`,
`leadstinger-cli stop-all`. A command starts a process per press, so it reacts a few
milliseconds later than a key; that's fine for most cues.

**Live keys with Companion.** For keys that show the tile's title, light up while it
plays and count down its remaining time, drive the Stream Deck from Bitfocus Companion
(next recipe).

## Bitfocus Companion, with countdowns

[Companion](https://bitfocus.io/companion) runs Stream Decks and other button surfaces and
talks OSC. LeadStinger listens for OSC and sends its state back (titles, colours,
playing state, remaining time), so a Companion key can fire a tile and show its
countdown.

1. Turn on OSC with feedback to Companion. Both run on this machine, so nothing listens
   on the network:

   ```bash
   leadstinger-cli osc.enable --port 9000 --feedback 127.0.0.1:9001
   ```

2. In Companion, import the page in
   [`examples/companion/`](../examples/companion/) (see its README for which version and
   module it is made for), or build your own keys with the generic OSC connection:
   target `127.0.0.1` port `9000`, listening on `9001`.
3. A key sends `/ls/tile/1/trigger` with 1 on press and 0 on release. It shows
   `/ls/tile/1/title`, and while `/ls/tile/1/state` is 1 it lights and shows
   `/ls/tile/1/time` (a countdown such as `0:12`). `/ls/now/time` counts down whatever
   the window's counter shows, for one big countdown key.
4. When Companion starts after LeadStinger, its keys fill in within five seconds, or at
   once if a key sends `/ls/resync`.

Every address is listed in [CLI.md](CLI.md#osc). The same addresses drive the
[TouchOSC](../examples/touchosc/) and [Open Stage Control](../examples/open-stage-control/)
layouts.

## OBS: LeadStinger on its own track

Recording or streaming with the sound effects on their own track lets you fix their
level afterwards, or leave them out of a clip.

**macOS 13 and later.** In OBS 30 or later, add a **macOS Audio Capture** source, choose
**Application Audio Capture** and pick LeadStinger. Then, in **Edit ▸ Advanced Audio
Properties**, tick only track 2 for that source (and untick track 2 for the
microphone). In **Settings ▸ Output ▸ Recording**, record tracks 1 and 2. LeadStinger
still plays through your speakers or headphones as usual.

**Linux (PipeWire).** Make a virtual output for LeadStinger (as in the next recipe),
send it there, and add an **Audio Output Capture (PulseAudio)** source in OBS with the
device set to that virtual output's monitor. Assign it to its own track as above. To hear
it too, connect the virtual output's monitor to your speakers in a patchbay such as
qpwgraph or Helvum.

**With an output of its own.** If your interface has spare channels, add an Output on
channels 3-4 (Settings ▸ Outputs, or `leadstinger-cli output.add`) and route the stream's
tiles to it; OBS then captures those channels from the interface.

## A virtual microphone

To play clips into Zoom, Discord, a browser call or a game, LeadStinger plays into a
virtual audio device that the other app uses as its microphone.

**macOS: BlackHole.** Install [BlackHole](https://existential.audio/blackhole/) (2ch is
enough; `brew install blackhole-2ch`). Add an Output for it, and send the tiles callers
should hear there:

```bash
leadstinger-cli output.add Calls --device "BlackHole 2ch"
leadstinger-cli tile.set "Air Horn" --output Calls
```

(or Settings ▸ Outputs ▸ Add Output, and the tile's Output in the inspector). In the call
app, choose **BlackHole 2ch** as the microphone. That sends only the clips: to send your
voice as well, make an **Aggregate Device** in Audio MIDI Setup with your microphone and
BlackHole and pick that in the call app. To hear the clips yourself, make a
**Multi-Output Device** with BlackHole and your headphones in Audio MIDI Setup and use it
as the Output's device instead.

**Linux: a PipeWire null sink.** Create a virtual output that lasts across reboots, in
`~/.config/pipewire/pipewire.conf.d/leadstinger-stream.conf`:

```
context.objects = [
    { factory = adapter
      args = {
          factory.name     = support.null-audio-sink
          node.name        = "leadstinger-stream"
          node.description = "LeadStinger stream"
          media.class      = Audio/Sink
          audio.position   = [ FL FR ]
      }
    }
]
```

and restart PipeWire (`systemctl --user restart pipewire`). LeadStinger plays through
ALSA, so give the sink an ALSA name it can choose, in `~/.asoundrc`:

```
pcm.leadstinger-stream {
    type pipewire
    playback_node "leadstinger-stream"
    hint { show on description "LeadStinger stream (PipeWire)" }
}
```

Restart LeadStinger, add an Output on it and route tiles there:

```bash
leadstinger-cli output.add Stream --device leadstinger-stream
leadstinger-cli tile.set "Air Horn" --output Stream
```
 Call apps list **Monitor of LeadStinger stream** among
their microphones; where an app hides monitors, pick it with `pavucontrol` (Recording
tab) while the call runs. For your voice and the clips together, add a second null sink,
link your microphone and the stream's monitor into it with `pw-link`, and use that.

## Event hooks with `watch`

`leadstinger-cli watch` prints one JSON line per event while the app runs:
`tile.started`, `tile.stopped`, `tile.finished`, `tile.paused`, `tile.resumed`,
`cue.changed`, `set.selected`, `master.changed`, `library.changed`, `mode.changed`.
Piped into a small script, it's a hook for anything:

```bash
leadstinger-cli watch | while read -r event; do
    kind=$(printf '%s' "$event" | jq -r .event)
    title=$(printf '%s' "$event" | jq -r '.title // empty')
    case "$kind" in
        tile.started)  echo "$(date +%T) on air: $title" >> ~/show-log.txt ;;
        tile.finished) [ "$title" = "Intro Theme" ] && obs-cmd scene switch "Main" ;;
    esac
done
```

(`obs-cmd` is one of several small OBS remote-control tools; any command works there.)
In fish:

```fish
leadstinger-cli watch | while read -l event
    set title (echo $event | jq -r '.title // empty')
    test (echo $event | jq -r .event) = tile.started; and echo "on air: $title"
end
```

For lights or video that should follow a tile exactly, a tile can also send OSC or MIDI
itself when it starts and stops: see **Messages** in the [README](../README.md).

## Vamp until cue

Walk-in music that loops until the host is ready, then plays out its ending: give the
tile a **vamp**, a section that loops until it's let go.

```bash
leadstinger-cli tile.set "Walk-in" --vamp-start 12 --vamp-end 28
```

(or tick **Vamp** on the inspector's Cue tab and drag its handles on the waveform). Start
it as usual. While it loops, the pad says **VAMPING**; the next press (a double-click, its
key or pad, or GO in the cue list) lets it go, and the pad says **PLAYING ON** while it
plays to the end. The press after that stops it. A Hold pad vamps while it's held and
plays on when it's let go.

In the cue list, GO lets a vamping cue play on and starts nothing else; the next GO
plays the next cue. To have the next cue start by itself when the music ends, set the
vamping cue's **Next cue** to Follow:

```bash
leadstinger-cli tile.set "Walk-in" --link follow
```

From another device, `leadstinger-cli tile.playOn "Walk-in"`, a Stream Deck key bound to
that command, or OSC `/ls/playon "Walk-in"` lets it go without touching the pad.
