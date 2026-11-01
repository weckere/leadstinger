# Control-surface layouts

Ready-made layouts for LeadStinger's OSC interface, written for this project:

| Folder | For | What you get |
|---|---|---|
| [`companion/`](companion/) | Bitfocus Companion (Stream Deck and other button surfaces) | a page of tile keys with titles, colours and countdowns, plus GO, Stop All and a big countdown |
| [`touchosc/`](touchosc/) | TouchOSC (phones and tablets) | 16 tile buttons with titles and playing state, GO, Stop All, master fader, talk-over |
| [`open-stage-control/`](open-stage-control/) | Open Stage Control (any browser) | the same, with tile colours and countdowns |

All three use only the shipped `/ls/...` addresses (listed in
[docs/CLI.md](../docs/CLI.md#osc)), so they work with any set: tile keys follow the
current set's first 16 tiles by position.

## Setting up LeadStinger

Turn on OSC and send feedback to the surface. With the surface on the same machine,
nothing listens on the network:

```bash
leadstinger-cli osc.enable --port 9000 --feedback 127.0.0.1:9001
```

For a phone or tablet running TouchOSC, OSC has to listen on the network, and feedback
goes to the device's address:

```bash
leadstinger-cli osc.enable --port 9000 --lan true --feedback 192.168.1.50:9001
```

OSC has no password: anyone on that network can fire sounds (never delete them), so use
it on a network you trust.

## What every layout does

| Control | Sends | Shows |
|---|---|---|
| Tile key N | `/ls/tile/N/trigger` 1 on press, 0 on release | `/ls/tile/N/title`; lit while `/ls/tile/N/state` is 1; `/ls/tile/N/time` counting down |
| GO | `/ls/go` | `/ls/cue/next/title` |
| Stop All | `/ls/stopall` | |
| Master | `/ls/master` 0-1 | `/ls/master` |
| Talk-over | `/ls/duck` 1 while held | `/ls/duck` |
| Countdown | | `/ls/now/title` and `/ls/now/time` |
| Resync | `/ls/resync` | |

A surface that connects after LeadStinger fills in within five seconds, or at once when
it sends `/ls/resync`.
