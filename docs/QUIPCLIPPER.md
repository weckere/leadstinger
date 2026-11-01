# Clips from QuipClipper

[QuipClipper](https://github.com/weckere/quipclipper) is a self-hosted web app that finds a
line in a movie, show or podcast by its subtitles and cuts it with ffmpeg. Its **Send to
LeadStinger** button hands the clip to LeadStinger with a link. LeadStinger downloads the
clip into a new tile whose trim is the line itself, while the clip keeps a couple of
seconds either side, so the cut can be fine-tuned in the inspector without going back to
QuipClipper. This file is the link format's reference, for QuipClipper and for anything
else that wants to send clips the same way (decisions D-076, D-081, D-082 and D-132 to D-135;
the code is `src/quip.rs` and `src/quip/`).

## Setting it up

1. Connect LeadStinger to the server once. Any of these does it, and none needs the
   server's API token typed in:
   - **Click Send to LeadStinger** on a clip in QuipClipper. LeadStinger comes forward and
     asks *"Connect to QuipClipper at nas.local:8896?"*, saying where the server is (on
     this computer, your local network, your tailnet, or the internet) and what it sent.
     **Connect and Import** connects it (below) and imports the clip; **Cancel** connects
     nothing, and links from that server are refused until the app restarts.
   - **Connect LeadStinger** in QuipClipper's settings opens a `leadstinger://pair` link,
     which asks the same way.
   - **Settings ▸ QuipClipper** in LeadStinger: a server found on the network has
     **Connect** (see [Finding QuipClipper on the network](#finding-quipclipper-on-the-network)),
     or type the address and click Connect with the token field empty.
   - From a script: `leadstinger-cli quip.pair https://quip.example` prints the code and
     waits for Allow.

   Connecting **pairs** with a QuipClipper that offers it: QuipClipper's page shows
   *"LeadStinger on laptop wants access, code 7F3K"* with **Allow** and **Deny**, and
   LeadStinger shows the same code while it waits. On Allow, QuipClipper gives this copy
   of LeadStinger a token of its own, which it lists under Settings ▸ Connected apps and
   can revoke there; Disconnect in LeadStinger revokes it too. A QuipClipper without
   pairing (0.3.0 and older) that has no password is connected **without a token**: it's sent
   no credentials at all. One with a password and no pairing needs its API token.

   The token way still works, for scripts and for a server with a password and no
   pairing: type the token into Settings ▸ QuipClipper beside the address, or

   ```bash
   leadstinger-cli quip.connect https://quip.example --token -   # reads the token from stdin
   ```

   `quip.connect` tries the token on the server (`GET /api/config`) before it keeps it;
   `--check false` skips that. Without `--token` it connects an open server (one with no
   password) without one. `quip.list` shows the connected servers (whether each has a
   token and was paired, never the token) and `quip.disconnect https://quip.example`
   forgets one. `leadstinger-cli --safe` refuses `quip.connect`, `quip.pair` and
   `quip.disconnect`: which servers to trust is the user's call.
2. Make sure the system opens `leadstinger://` links with LeadStinger:
   - **macOS**: the app registers the scheme itself (its `Info.plist`). Keep the app in
     Applications, or open it once, so macOS knows about it.
   - **Linux, Nix package or NixOS module**: the launcher entry carries
     `MimeType=x-scheme-handler/leadstinger;`, so desktops that read it need nothing more.
     If a browser still asks, run
     `xdg-mime default leadstinger.desktop x-scheme-handler/leadstinger`.
   - **Linux, AppImage or tarball**: run `leadstinger --register-links` once (from the
     AppImage: `./LeadStinger-….AppImage --register-links`). It writes a hidden launcher
     entry for links to `~/.local/share/applications/leadstinger-links.desktop` and makes
     it the default.

## The link

```text
leadstinger://quip?server=<base URL>&clip=<clip path>&title=<text>&start=<s>&end=<s>&set=<name>
```

Every value is percent-encoded, as JavaScript's `encodeURIComponent` does (a `+` reads as
a space, so a literal `+` must be `%2B`). For example:

```text
leadstinger://quip?server=https%3A%2F%2Fquip.example&clip=Heat%2FHeat%20-%20attached.mkv&title=Don%27t%20get%20attached&start=2&end=5.4&set=Movie%20Lines
```

| Parameter | | Meaning |
|---|---|---|
| `server` | required | QuipClipper's base URL, `http` or `https`, with an optional path prefix (`https://example.org/quip`) and no user name, password, query or fragment. It must match a connected server: case, a trailing `/` and the default port don't matter. |
| `clip` | required | The clip's path under QuipClipper's clips folder, as `/api/clips` lists it (`Heat/Heat - attached.mkv`): relative, `/`-separated, with no empty, `.` or `..` parts, no backslash and no control characters, at most 1024 bytes. Its extension says what it is. |
| `title` | optional | The tile's title, 1 to 200 characters on one line. Default: the clip's file name without its extension. |
| `start` | optional | Seconds into the clip where the line starts: the tile's trim start. Default 0. |
| `end` | optional | Seconds into the clip where the line ends: the tile's trim end, after `start`. Default: the end of the clip. |
| `set` | optional | The name of the set to put the tile in, 1 to 100 characters; a set of that name is created if there's none. Default: the current set. |
| `as` | optional | `flac`: download the clip converted to FLAC by the server (`?as=flac`), every channel kept. Default: the clip as it is. |

`start` and `end` are numbers from 0 to 86400, written with a `.` for decimals. Anything
else is refused, and the whole link with it: an unknown parameter, one given twice, a
fragment, or anything after `leadstinger://quip` other than the query.

LeadStinger then downloads `<server>/api/clips/download/<clip>` (each path part
percent-encoded) with the server's token, as `X-API-Key: <token>` and as the `api` user's
basic-auth password (`Authorization: Basic base64(api:<token>)`), which also gets through
QuipClipper's optional password gate. The tile appears at once, at the end of the set,
showing "Downloading 34%"; Cancel Import stops it. A clip in a video container (MKV, MP4,
MOV, ...) or AC-3/DTS then goes through ffmpeg like any import, keeping every channel.
Importing is one undo step.

A `clip` ending `.lsset` (any case) is a set file: the link imports the whole set, as
File ▸ Import Set does, instead of one tile. LeadStinger downloads it the same way, with
the server's token, to a temporary file in its data folder, imports it (on a thread while
the window is open, with a "Downloading the set …" notice) and removes the download. Such
a link takes only `server` and `clip`: `title`, `start`, `end`, `set` and `as` (and
`quip.import`'s `--notes`) are refused, since the set file carries its own name, tiles and
trims. A link from a server that isn't connected, or with Ask before each import on, asks
first as for a clip ("… sent the set “Lines”. Import it?"). QuipClipper opens such a link
for **Send to LeadStinger** with several lines ticked: it builds
`LeadStinger/<name>.lsset` in its clips library and sends that.

`leadstinger-cli quip.import` does the same from a script. It takes a link, or a clip's
download URL plus any of the other parameters:

```bash
leadstinger-cli quip.import 'leadstinger://quip?server=https%3A%2F%2Fquip.example&clip=a.wav'
leadstinger-cli quip.import https://quip.example/api/clips/download/Heat/attached.mkv --start 2 --end 5.4 --set "Movie Lines"
```

A clip URL may end `?as=flac` (QuipClipper's `flac_url`), and `--as flac` asks for the
same: the server converts the clip to FLAC first, every channel kept, so video and
AC-3/DTS clips import without ffmpeg. `--notes` sets the tile's notes.

## Browsing the clips library

File ▸ **Browse QuipClipper…** (or **Browse** beside a server in Settings ▸ QuipClipper)
opens a window onto a connected server's clips library, through QuipClipper's read-only
clips API (contract `quipclipper.clips/1`, in QuipClipper's docs/API.md):

- **Folders** as a breadcrumb (Clips › folder › folder) and rows; a **search** box that
  looks in the folder and every folder under it once you stop typing; **All / Audio /
  Video / Sets** filters; **Newest first**.
- Each clip shows its **line** (the sidecar's `cue`), where it's from (the show and
  `S01E04` with the episode's title, or the movie and its year), its length, a **5.1**
  badge for surround, **Preview** and **Add**.
- **Preview** plays the clip on the Preview output (Settings ▸ Outputs; on Main when
  there's none) from memory: nothing is saved. Audio the app reads plays from
  `/api/clips/stream/…`; video and AC-3 clips from the server's FLAC.
- **Add**, a double click, Enter, or a drag onto the board puts the clip at the end of
  the current set: downloaded as FLAC (when the server offers it), titled with the line,
  trimmed to the line (`meta.trim`; a backfilled clip without one isn't trimmed), with
  notes such as "Heat (1995) · at 1:02:03 · QuipClipper". A **set file** (`.lsset`)
  is imported as a set.
- **N new** counts the clips cut since you last looked; clicking it lists them and
  counts them as seen. With several servers, the server picker shows each one's count.
- **While the window is closed** the status bar shows how many clips are new over all
  servers ("3 new in QuipClipper"; in the Panel theme an amber block at the end of the
  bottom bar). Click it to open the window on them. The app asks each server every 30
  seconds while the window is open and every minute while it's closed, a cheap request
  that costs an unchanged library a `304`. A server that can't be reached is asked less
  and less often (up to every 15 minutes) and only its hover text says so: nothing pops
  up during a show. Turn it off in Settings ▸ QuipClipper ▸ Show new clips in the status
  bar (`settings.quipBadge --on false`); the window still counts while it's open.
- From the **keyboard**: Up and Down move along the clips, Enter adds, Space previews.
  Screen readers hear a list named "Clips", each row read as "line, where it's from,
  audio clip, 2.0 seconds, 5.1".

The same from the command line (each takes `--server` when more than one is connected):

```bash
leadstinger-cli quip.browse --folder "Heat (1995)"           # a folder's clips, newest first
leadstinger-cli quip.search "get attached" --kind audio     # every clip with those words
leadstinger-cli quip.new                                    # clips cut since last time
leadstinger-cli quip.new --mark true                        # ... and count them as seen
leadstinger-cli status --json | jq .quip_new                # what the badge counts, per server
leadstinger-cli view.quip --new true                        # open the window on them (seen)
leadstinger-cli quip.add "Heat (1995)/a_00-01-05_Attached.mka" --set "Movie Lines"
leadstinger-cli quip.preview "Heat (1995)/a_00-01-05_Attached.mka"   # needs the app
```

`quip.new` keeps its place (QuipClipper's `cursor`) per server in `quip-servers.json`,
beside the token, never in the library. The first time it looks at a server it only
notes where the library is, so nothing already there counts as new.

Every request goes to the connected server's own address, with its token: the listing's
`download_url` and `stream_url` can point somewhere else (a front proxy) and are never
used. Listings are cached with their ETag and asked for again with `If-None-Match`, so an
unchanged library costs the server a `304`.

## Pairing

The pairing contract is QuipClipper's `quipclipper.pair/1` (its docs/API.md, Pairing):

1. `GET <server>/api/config`, with no credentials: is it QuipClipper, has it a password
   (`auth_required`), does it pair (`pairing`), and its `server_id`.
2. `POST <server>/api/pair` with `{"name": "LeadStinger", "host": "<this machine>"}` and the
   CSRF header, no credentials → `{id, code, poll_secret, expires_in}`. A 404 means no
   pairing: an open server is connected without a token, one with a password needs its
   token.
3. Every 2 seconds, `GET <server>/api/pair/<id>` with `X-Pair-Secret: <poll_secret>` →
   `pending`, `approved` (with the token, once), `denied` or `expired`. Cancel stops
   waiting; a token that arrives after Cancel is revoked at once.
4. The token and QuipClipper's id for this pairing (`client_id`) go in `quip-servers.json`
   like any token. Disconnect sends `DELETE <server>/api/clients/self` with it.

`quip.pair` returns at once (`{status: starting}`) and `quip.pairing` says how it's going
(`waiting` with the `code` and the `approve_url` to open, then `connected`, `denied`,
`expired`, `cancelled` or `failed` with a `message`); `leadstinger-cli quip.pair` waits for
the answer itself. `quip.pair {cancel: true}` stops waiting.

## Finding QuipClipper on the network

A QuipClipper with advertising turned on (its `QC_ADVERTISE`, off by default; see its
docs/DEPLOY.md) answers mDNS/Bonjour questions for `_quipclipper._tcp`, with its address
(TXT `url`) and `server_id` (TXT `id`). LeadStinger looks for a few seconds:

- when Settings ▸ QuipClipper or the QuipClipper window opens with no server connected
  (at most once a minute),
- when you click **Find on Network** there, or
- `leadstinger-cli quip.discover` (it waits for the look and lists what answered).

It never looks in the background. Each answer is checked (`GET <url>/api/health`, no
credentials, must say `quipclipper-web`) and listed as *"QuipClipper on nas
(192.168.1.20)"*, with "has a password" when its check needed one, unless it's connected
already (same address or same `server_id`). Connect pairs with it as above. On macOS the
first look brings up the system's Local Network permission prompt.

A QuipClipper that's deliberately reachable only on a tailnet or behind a VPN should leave
advertising off: an advertisement tells the whole local network it's there.

## Safety

- **Only connected servers**, and nothing on the network before you say so. A link naming
  a server LeadStinger isn't connected to is refused before anything goes over the network
  (code `not_connected`), with a message saying how to connect it, unless the system opened
  it while the window is up: then it asks first, and only Connect sends anything to the
  server. So a link on some web page can't make LeadStinger download from anywhere you
  didn't choose, or send a token there.
- **The prompt can't be answered by accident.** Cancel has the keyboard focus, so Space
  (GO) or Enter, pressed for the board, cancels. Connect takes a click (or a Tab to it).
  One prompt at a time: a link for another server while one is up is refused, and so is
  a link from a server you cancelled, until the app restarts. The prompt says where the
  server is and warns about plain http on the internet. Scripts (`quip.import` without
  `ask`), `leadstinger-cli serve` and `--safe` are refused as before, never asked.
- **No credential goes to a new server.** Pairing sends only the app's name and the
  machine's host name; the token comes *from* the server. An open server connected without
  a token gets no `X-API-Key` or `Authorization` header at all.
- **Found on the network is only a suggestion.** Anyone on a network can answer an mDNS
  question, so the list shows the address each answer came from, and connecting is still
  Connect and then Allow in QuipClipper. Allow in *your own* QuipClipper page (a bookmark,
  not a link from somewhere): a stranger's fake QuipClipper can't put a request there.
- **Ask before each import**: Settings ▸ QuipClipper ▸ Ask before importing each
  QuipClipper link (`settings.quipAskEachImport`, off by default) has every opened link
  ask *"Import from QuipClipper?"*, even from a connected server.
- **http and https only**, and redirects are followed only within the same server (and
  from http to https on the same host), at most five; a redirect to another host is
  refused. Pairing's requests follow no redirect at all.
- **At most 1 GB** per clip; a web page or JSON where a clip should be is refused.
- **The tokens** are kept in `quip-servers.json` in the library's data folder, readable
  only by you (0600; tightened again if something loosens it), never in `Library.json` or
  a set file, and never shown in messages, errors or `quip.list`. `--token -` keeps a
  token off the command line, where other users' `ps` would see it. A server connected
  without a token is stored without one (an older LeadStinger can't read that file: it
  asks you to remove it and connect again).
- **Certificates** are checked against the system's trusted ones (the macOS keychain; the
  distribution's CA bundle on Linux).

## Opening a link

- **macOS** sends the link to LeadStinger as an Apple Event: to the running app, or to the
  app it starts, which imports the link (or asks, as above) once its window is up.
- **Linux** starts `leadstinger <link>`. With the app already running on the same library,
  that second launch passes the link to it over the control socket (docs/CLI.md) and
  exits; otherwise the new app imports it once its window is up.
- Either way the window comes to the front and the status bar says what happened,
  including a refusal.
