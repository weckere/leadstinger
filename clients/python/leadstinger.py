""""LeadStinger from Python: every command in the app, as a method.

Generated from the command registry by `leadstinger-cli commands --python`; don't edit it,
regenerate it. Needs Python 3.8 or newer and nothing else, plus `leadstinger-cli` on the
PATH (or `cli=` / LEADSTINGER_CLI naming it).

    import leadstinger
    with leadstinger.connect() as ls:                  # the running app, else the library
        ls.tile.trigger("Rimshot")
        print(ls.status()["playing"])
        with ls.batch() as b:                          # one undo step; nothing kept on failure
            for i, t in enumerate(ls.tile.list(set="Show")):
                b.tile.set(t["id"], title=f"{i + 1:02} {t['title']}")
        print(b.result["changes"])

Commands take the registry's parameter names as keyword arguments (`fadeIn=0.5`), and the
main parameter (the tile, the set…) may also be given first, without its name. A command
that fails raises `leadstinger.Error` with the app's `code`, `message` and `candidates`.
`connect(safe=True, allow_dirs=[...])` refuses what `leadstinger-cli --safe` refuses, and
`connect(ssh="user@host")` goes through a remote `leadstinger-cli --ssh` forced command.
docs/CLI.md has the commands; `ls.call(id, **params)` runs any of them by id.
"""

import json
import os
import subprocess

__all__ = ["connect", "Error", "LeadStinger"]

_UNSET = object()


class Error(Exception):
    """A command that didn't run: `code` is stable (not_found, ambiguous, refused,
    not_running, …), `candidates` lists the matches of an ambiguous name, and `result` is a
    failed batch's whole reply."""

    def __init__(self, code, message, candidates=None, result=None):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message
        self.candidates = candidates or []
        self.result = result


class _Process:
    """A `leadstinger-cli connect` process: one JSON request per line, one reply each."""

    def __init__(self, argv):
        self._argv = argv
        self._p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self._next = 0

    def call(self, command, params):
        self._next += 1
        request = {"id": self._next, "command": command, "params": params}
        try:
            self._p.stdin.write(json.dumps(request) + "\n")
            self._p.stdin.flush()
            line = self._p.stdout.readline()
        except (BrokenPipeError, OSError) as e:
            raise Error("io", f"lost {self._argv[0]} connect: {e}") from e
        if not line:
            raise Error("io", f"{' '.join(self._argv)} ended (exit {self._p.wait()})")
        reply = json.loads(line)
        if reply.get("ok"):
            return reply.get("result")
        e = reply.get("error") or {}
        raise Error(e.get("code", "failed"), e.get("message", "the command failed"), e.get("candidates"))

    def close(self):
        if self._p.poll() is None:
            self._p.stdin.close()
            self._p.wait()


class _Recorder:
    """A batch being written: commands are kept as steps, not sent."""

    def __init__(self):
        self.steps = []

    def call(self, command, params):
        if command == "batch.run":
            raise Error("bad_params", "a batch can't hold another batch")
        self.steps.append({"command": command, "params": params})

    def close(self):
        pass


class _Batch:
    def __init__(self, conn, atomic, dry_run):
        self._conn = conn
        self._recorder = _Recorder()
        self._options = {"atomic": atomic, "dryRun": dry_run}
        self._steps = LeadStinger(self._recorder)

    def __enter__(self):
        return self._steps

    def __exit__(self, kind, value, tb):
        if kind is not None:
            return False  # an exception in the block: nothing is sent
        result = self._steps.result = self._conn.call("batch.run", dict(self._options, steps=self._recorder.steps))
        if not result.get("ok"):
            step = next((s for s in result.get("steps", []) if not s.get("ok")), {})
            e = step.get("error") or {}
            raise Error(e.get("code", "failed"), f"step {step.get('step')} ({step.get('command')}): {e.get('message', 'failed')}",
                        e.get("candidates"), result=result)
        return False


def _main(params, name, value):
    if name in params:
        raise TypeError(f"{name} given twice: first, and as {name}=")
    params[name] = value


class _Area:
    __slots__ = ("_conn",)

    def __init__(self, conn):
        self._conn = conn

class _Library(_Area):
    """The `library.*` commands."""

    def get(self, **params):
        """Show the whole library.

        → the library as stored in Library.json

        `library.get`; read-only
        """
        return self._conn.call("library.get", params)

    def path(self, **params):
        """Show where the library is.

        → {library, data_dir}

        `library.path`; read-only
        """
        return self._conn.call("library.path", params)

    def reveal(self, **params):
        """Show Library Folder.

        {} opens the data folder in the file manager

        `library.reveal`; needs the running app
        """
        return self._conn.call("library.reveal", params)


class _Tile(_Area):
    """The `tile.*` commands."""

    def trigger(self, tile=_UNSET, /, **params):
        """Trigger a tile.

        {tile, event?: press|release (default press), toggle? (true: a press while it plays
        stops it even when it's One-shot, as Enter on a focused pad does)} as its hotkey
        would, following its options (hold, pausable, oneShot)

        `tile.trigger`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.trigger", params)

    def play(self, tile=_UNSET, /, **params):
        """Play a tile.

        {tile, restart? (default false: a playing tile keeps going)}

        `tile.play`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.play", params)

    def stop(self, tile=_UNSET, /, **params):
        """Stop a tile.

        {tile, fade? (seconds; default the tile's fade-out)}

        `tile.stop`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.stop", params)

    def jump(self, tile=_UNSET, /, **params):
        """Jump to a marker.

        {tile, marker (name or #n)} a playing tile moves there with a short crossfade (past
        the vamp, it plays on); a stopped one starts there → {tile, marker, at}

        `tile.jump`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.jump", params)

    def marker_add(self, tile=_UNSET, /, **params):
        """Add a marker.

        {tile, at? (seconds into the file; default: where it's playing now), name? (default
        "Marker N")}

        `tile.markerAdd`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.markerAdd", params)

    def marker_move(self, tile=_UNSET, /, **params):
        """Move or rename a marker.

        {tile, marker (name or #n), at? (seconds into the file), name? (a new name)}

        `tile.markerMove`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.markerMove", params)

    def subtitles(self, tile=_UNSET, /, **params):
        """Show a tile's subtitles.

        {tile} its subtitle lines (QuipClipper's cues), in seconds into its audio file
        whatever its trim → {tile, title, cues: [{start, end, text, words? ([{start, end,
        text}]), aligned? (true: the words were placed by hand), added? (true: the line was
        added in the editor)}], cues_source ({kind, lang?, ...} or null), heard (how many
        lines overlap the trim), trim_start, trim_end}

        `tile.subtitles`; read-only
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitles", params)

    def subtitles_set(self, tile=_UNSET, /, **params):
        """Set a tile's subtitles.

        {tile, cues ([{start, end, text, words?, aligned?, added?}] as tile.subtitles gives
        them, in seconds into the audio file), cues_source? ({kind, lang?, ...}),
        keepAligned? (default true: a line aligned by hand whose text matches a new line
        overlapping it stays as it is)} lines that can't be read are skipped (at most 2000
        lines of 500 characters, 200 words each) → {tile, lines, skipped, skipped_words,
        kept_aligned}

        `tile.subtitlesSet`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitlesSet", params)

    def transcribe(self, tile=_UNSET, /, **params):
        """Transcribe.

        {tile | tiles (default the selection), engine? (auto|whisper|quip|apple; default
        settings.transcribe's), lang? (a language code, e.g. en; default the setting, else
        the engine's own), gaps? (default true: also add what's heard between a tile's
        subtitle lines)} listens to each tile's whole sound: a tile without subtitles (or
        with only machine-made ones) gets the lines heard, marked machine-made {kind:
        machine, engine, model, lang}; a tile with subtitles keeps its text, and its lines
        without word timings get them from what was heard. With gaps, speech heard where
        none of its lines is spoken (a stretch of 0.5 s or more; a line's first or last word
        heard up to 0.25 s outside it is still that line's) becomes new lines, each marked
        machine: {kind: machine, engine, model, lang}, never over a known line; a later run
        replaces the gap lines it added, except ones changed by hand since. Each word's
        start is then moved onto the sound it starts on (up to 0.15 s); lines aligned by
        hand stay as they are. Runs in the background, one tile at a time; the CLI waits for
        it. With quip, the sound is sent to the connected QuipClipper (settings.transcribe's
        quipServer, else the first by address that takes uploads), which keeps none of it.
        Refused for a locked set, and with code unavailable when no engine can run (the
        reason says what to install) → {job, tiles, engine, sendsTo ("QuipClipper at …"
        when the audio leaves this computer, else null), skipped: [{tile, title, reason}]};
        each tile's result (transcribe.status) has gaps: {added, replaced, kept}

        `tile.transcribe`; changes the library; works with the app closed
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.transcribe", params)

    def align_words(self, tile=_UNSET, /, **params):
        """Align Words.

        {tile, line? (from 1: listen to that line, a little either side, and time its words
        again even if it has some; default every line without word timings), engine?, lang?}
        the line's text stays; its words take the times the engine heard them at, the ones
        it missed are spread between, and Reset puts the old timing back. A line aligned by
        hand is refused → as tile.transcribe

        `tile.alignWords`; changes the library; works with the app closed
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.alignWords", params)

    def subtitles_clear(self, tile=_UNSET, /, **params):
        """Remove a tile's subtitles.

        {tile} → {tile, removed (how many lines)}

        `tile.subtitlesClear`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitlesClear", params)

    def subtitle_edge(self, tile=_UNSET, /, **params):
        """Move a subtitle word's edge.

        {tile, line (from 1), word (from 1), edge: start|end, at (seconds into the file),
        split? (default false: a boundary shared with the next or previous word moves that
        word's edge too; true leaves it)} words keep their order and at least 20 ms; marks
        the line aligned → {tile, line, start, end, text, aligned, provisional, words:
        [{start, end, text}]}

        `tile.subtitleEdge`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleEdge", params)

    def subtitle_nudge(self, tile=_UNSET, /, **params):
        """Nudge a subtitle word.

        {tile, line, word, grip: start|end|word, by (seconds; the editor's arrow keys send
        0.01, with Shift 0.1)} moves its start edge, its end edge (a shared boundary moves
        the neighbour's edge too) or the whole word → the line

        `tile.subtitleNudge`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleNudge", params)

    def subtitle_word_move(self, tile=_UNSET, /, **params):
        """Move a subtitle word.

        {tile, line, word, at (its new start) | by (seconds), split?} keeps its length;
        shared boundaries take the neighbours' edges along (unless split), gaps stop it →
        the line

        `tile.subtitleWordMove`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleWordMove", params)

    def subtitle_tap(self, tile=_UNSET, /, **params):
        """Start a subtitle word here.

        {tile, line, word, at (seconds into the file, e.g. where it's playing)} the word's
        start moves there, or the whole word when that's past its end: tapping a line in
        while it plays → the line

        `tile.subtitleTap`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleTap", params)

    def subtitle_line_move(self, tile=_UNSET, /, **params):
        """Move a subtitle line.

        {tile, line, at (its new start) | by (seconds)} all its words with it, inside the
        audio → the line (and its new number: lines stay in time order)

        `tile.subtitleLineMove`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleLineMove", params)

    def subtitle_auto_align(self, tile=_UNSET, /, **params):
        """Auto-align a subtitle line.

        {tile, line} a suggestion: each word starts where a sound rises inside the line (the
        first and strongest rises when there are more than words; the even split when there
        are fewer); not marked aligned → the line, method: onsets|strongest|even

        `tile.subtitleAutoAlign`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleAutoAlign", params)

    def subtitle_reset(self, tile=_UNSET, /, **params):
        """Reset a subtitle line.

        {tile, line} its start, end and words back as the source gave them (or as they were
        before its first edit here) → the line, reset (false: it was never changed)

        `tile.subtitleReset`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleReset", params)

    def subtitle_word_text(self, tile=_UNSET, /, **params):
        """Retype a subtitle word.

        {tile, line, word, text} fixes its spelling; text with spaces splits it, its time
        shared out by length → the line

        `tile.subtitleWordText`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleWordText", params)

    def subtitle_word_join(self, tile=_UNSET, /, **params):
        """Join two subtitle words.

        {tile, line, word} the word and the next become one, spanning both → the line

        `tile.subtitleWordJoin`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleWordJoin", params)

    def subtitle_text(self, tile=_UNSET, /, **params):
        """Split a subtitle line again.

        {tile, line, text? (new text for the line; a line break is \\n)} drops its word
        timings, so its words are spread evenly over it until they're aligned again → the
        line

        `tile.subtitleText`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleText", params)

    def subtitle_add(self, tile=_UNSET, /, **params):
        """Add a subtitle line.

        {tile, text (a line break is \\n), start? (seconds into the file, with end; or give
        at), end? (seconds into the file), at? (instead of start and end: its start; it
        lasts about 0.3 s a word, 0.8 to 6 s, stopping before the next line), words? (onsets
        (default): each starts where a sound rises inside it, as Auto-Align does | even:
        spread evenly)} a line made here, marked added: a new copy of the subtitles from
        their source keeps it. In the open editor it becomes the selected line → the line,
        method: onsets|strongest|even, lines (how many now)

        `tile.subtitleAdd`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleAdd", params)

    def subtitle_delete(self, tile=_UNSET, /, **params):
        """Delete a subtitle line.

        {tile, line} removes it; a new copy from the source brings a line it has back →
        {tile, line, removed: {start, end, text, added}, lines (how many are left)}

        `tile.subtitleDelete`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.subtitleDelete", params)

    def marker_remove(self, tile=_UNSET, /, **params):
        """Remove a marker.

        {tile, marker (name or #n) | all: true}

        `tile.markerRemove`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.markerRemove", params)

    def play_on(self, tile=_UNSET, /, **params):
        """Play On.

        {tile} a tile looping its vamp section lets it go and plays on to the end (a press
        on a vamping tile does the same) → {tile, played_on}

        `tile.playOn`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.playOn", params)

    def release(self, tile=_UNSET, /, **params):
        """Play On (deprecated).

        {tile} deprecated (D-156): the old name of tile.playOn → {tile, played_on,
        released}

        `tile.release`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.release", params)

    def pause(self, tile=_UNSET, /, **params):
        """Pause a tile.

        {tile}

        `tile.pause`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.pause", params)

    def resume(self, tile=_UNSET, /, **params):
        """Resume a paused tile.

        {tile}

        `tile.resume`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.resume", params)

    def random(self, set=_UNSET, /, **params):
        """Play a Random Tile.

        {set? (default current; "*" for every set), color? (only tiles of this colour)}
        every tile in the pool plays once before any repeats, so never the last pick;
        disabled, empty and action tiles are left out; the Random button (D-171); the status
        bar says what it played → {tile, title, left}

        `tile.random`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("tile.random", params)

    def preview(self, tile=_UNSET, /, **params):
        """Preview a tile.

        {tile, from?, to? (seconds into the file: just that stretch, once, without its vamp,
        loop, fades or link; the editor's Play Selection)} play it, or stop it quickly if
        it's sounding (the context menu's Play/Stop); with Preview to on and its device not
        open, nothing plays (code preview_unavailable), never on Main

        `tile.preview`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.preview", params)

    def render(self, path=_UNSET, /, **params):
        """Export Audio.

        {tile? | tiles? (default the selection), path? (the file to write, or a folder for
        several tiles; without it the app opens the Export Audio window), format? (wav|flac;
        default the path's extension, else wav), rate? (Hz, 8000-192000; default the
        clip's)} the tile with trim, fades, volume, gain, speed, pitch and channel mix
        applied, once through (24-bit, stereo); runs in the background and the CLI waits for
        it; the library is untouched → {job, format, files, skipped}

        `tile.render`; read-only
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("tile.render", params)

    def list(self, set=_UNSET, /, **params):
        """List tiles.

        {set? (default current; "*" for all), query? (the words a search finds, as
        tile.search matches them)} → [{id, title, set, position, row, slot (counting
        blanks; column is the same), key, loop, solo, pausable, one_shot, hold, disabled,
        duration, ...}]

        `tile.list`; read-only
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("tile.list", params)

    def search(self, query=_UNSET, /, **params):
        """Search tiles.

        {query (words; every one must be found, in any order, ignoring case and accents),
        set? (default every set, the current one first)} searches each tile's title, the
        name of the file it was made from, its notes and its subtitles → [{tile, title,
        set, position, matched: [{field: title|file|notes|subtitles, text (the line that
        matched)}]}]

        `tile.search`; read-only
        """
        if query is not _UNSET:
            _main(params, "query", query)
        return self._conn.call("tile.search", params)

    def get(self, tile=_UNSET, /, **params):
        """Show a tile.

        {tile} → every setting of the tile

        `tile.get`; read-only
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.get", params)

    def add(self, paths=_UNSET, /, **params):
        """Add Sounds.

        {paths? ([file] or file; without it the app opens a file picker), set?, at? (1-based
        position, joining the row of the tile there) | row, slot? (that slot of the row: a
        blank one fills, one past the row's end opens blanks up to it, otherwise the slots
        from there move along) | at? (the gap before this slot; default the row's end),
        newRow? (true: a new row at row)} → {added: [id], errors}; files are copied in
        (AC-3, DTS and video converted with ffmpeg) in the background

        `tile.add`; changes the library; works with the app closed; one undo step
        """
        if paths is not _UNSET:
            _main(params, "paths", paths)
        return self._conn.call("tile.add", params)

    def paste(self, **params):
        """Paste Sounds.

        {} import files copied in a file manager (also ⌘/Ctrl+V outside text fields)

        `tile.paste`; needs the running app; one undo step
        """
        return self._conn.call("tile.paste", params)

    def replace_audio(self, path=_UNSET, /, **params):
        """Replace Audio.

        {tile, path? (without it the app opens a file picker)}

        `tile.replaceAudio`; changes the library; works with the app closed; one undo step
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("tile.replaceAudio", params)

    def duplicate(self, tile=_UNSET, /, **params):
        """Duplicate tile.

        {tile} → {id}

        `tile.duplicate`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.duplicate", params)

    def remove(self, tile=_UNSET, /, **params):
        """Remove tile.

        {tile}

        `tile.remove`; changes the library; works with the app closed; one undo step; destructive: refused under safe=True
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.remove", params)

    def move(self, tile=_UNSET, /, **params):
        """Move tile.

        {tile | tiles ([tile], all from one set: they move together as one run in the set's
        order, one undo step; what dragging a selected tile does), to (1-based position in
        its set; it joins the row of the tile it displaces; with tiles, where the first ends
        up) | row (1-based, as the rows are before the move), slot? (the slot it ends up in:
        a blank one fills, one past the row's end opens blanks up to it, otherwise the slots
        from there move along) | at? (the gap it goes into, numbered with it already out:
        where it ends up; default the row's end), newRow? (true: a new row of its own at
        row; rows + 1 for the end), leaveBlank? (true: a blank stays in its old slot, the
        slots keep the numbers they have now and nothing else moves; what dragging a tile
        onto a blank of its own row or past the row's end does), set? (another set to move
        it into, at its end unless to or row says where; what dropping on a set in the
        sidebar does)} otherwise its old row closes up; a row it leaves with no tiles goes;
        never into or out of a locked set → {position, row, slot, at, set? (when it
        changed sets)}, or with tiles {set, tiles: [{tile, position, row, slot}]}

        `tile.move`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.move", params)

    def copy(self, tile=_UNSET, /, **params):
        """Copy tiles.

        {tile | tiles ([tile], all from one set: copied as one run in the set's order, one
        undo step), to (1-based position in the set it goes into; the copies join the row of
        the tile there) | row (1-based, as the rows are now: the originals stay), slot? (the
        slot the first copy goes into: a blank one fills, one past the row's end opens
        blanks up to it, otherwise the slots from there move along) | at? (the gap they go
        into; default the row's end), newRow? (true: a new row of their own at row; rows + 1
        for the end), set? (another set to copy them into, at its end unless to or row says
        where; what Alt-dropping on a set in the sidebar does)} full copies (settings,
        colour, picture, options) that leave the originals where they are; a hand-set key
        goes with a copy only into another set where no tile has it; what Alt-dragging tiles
        does; never into or out of a locked set → {set, tiles: [{tile (the copy), copyOf,
        position, row, slot}]}

        `tile.copy`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.copy", params)

    def set(self, tile=_UNSET, /, **params):
        """Change tile settings.

        {tile | tiles ([tile]: change them all as one undo step; not hotkey), title?, emoji?
        (an emoji or a few, drawn before the title on the pad; emoji only; "" or null clears
        it), color?
        (red|orange|yellow|lime|green|mint|teal|cyan|blue|indigo|purple|pink|rose|crimson|brown|gray),
        hotkey? (a key on the keyboard's four rows, 1–= q–] a–' z–/, or with alt+
        (Option on macOS) e.g. alt+1; set by hand, it stays with the tile wherever it moves;
        null: no key; auto: its slot's key again, which follows it), hold? (plays while
        held, stops on release; with pausable, pauses), pausable? (a press while it plays
        pauses it, the next resumes), oneShot? (a press while it plays starts it again from
        the beginning; pausable and oneShot exclude each other: turning one on turns the
        other off), mode? (deprecated alias for the three: toggle = none, pause = pausable,
        restart = oneShot, hold = hold), volume? (0-1), gainDb? (-24 to +24), output? (an
        output's name, or main: where it plays first), sends? ([{output, gainDb?}] or
        "Main,Stream": every output it plays on, each at its own level; null = Main only),
        channels? (files with more than two channels: auto (the stereo downmix), or one
        route per channel in file order, as "left,right,both:-3,mute,left:-3,right:-3" or
        [{to: left|right|both|mute, gainDb?}]), speed? (0.5-2; the pitch follows), pitch?
        (semitones, -12 to +12, keeping the speed; rendered once in the background), duck?
        (none|ducks|ducked), duckLevel? (a bed's own duck level, dB below its level, -40 to
        0; default or null: the library's duck amount), fadeIn?, fadeOut? (s: the fade at
        the end of the clip), stopFade? (s: the fade when it's stopped; 0 = at once, with
        the 0.1 s de-click every stop gets), preWait? (s between triggering it and its
        start), link? (none|continue|follow: what starts the next cue in the set without GO:
        continue = postWait s after this one starts, follow = postWait s after it ends),
        postWait? (s), fadeCurve? (gentle|linear|equal_power|exponential|s_curve), loop?,
        solo?, trimStart?, trimEnd? (s; null = to the end), vampStart?, vampEnd? (s, inside
        the trim: a section that loops until the next press, GO or tile.playOn lets it play
        on), vamp? (null clears it), pictureLayer? (underlay|overlay), pictureFit?
        (fill|fit), pictureOpacity? (0-1), onStart?, onStop? (an OSC or MIDI message, or a
        list of them, sent when the tile starts or stops: osc://host:port/address args,
        midi:note:60:ch1:vel100 @port, midi:noteoff:60, midi:cc:20:val127, midi:program:5;
        null clears), notes? (text shown in the cue list), disabled? (triggers and GO skip
        it), muted? (plays at zero gain on every send, keeping its volume, timing, cues,
        ducking and Solo)}

        `tile.set`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.set", params)

    def enable(self, tile=_UNSET, /, **params):
        """Enable tile.

        {tile} triggers, bindings and GO play it again

        `tile.enable`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.enable", params)

    def disable(self, tile=_UNSET, /, **params):
        """Disable tile.

        {tile} triggers, bindings and GO skip it (a playing tile plays on; Preview still
        works); it draws dimmed

        `tile.disable`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.disable", params)

    def add_action(self, kind=_UNSET, /, **params):
        """New Action Tile.

        {kind? (play|fade|stop|pause|play_on; release is play_on's old name; default fade),
        targets? ([tile, or "set:Name" for every tile in a set; add @seconds for a group's
        offset, e.g. "Thunder@1.5"]), fade? (seconds, 0-60; default 3), title?, set?, at?
        (1-based position)} a tile with no audio that acts on other tiles when triggered or
        GO'd, as theatre fade and stop cues do; kind play makes a group that fires its
        targets, each after its offset → {id}

        `tile.addAction`; changes the library; works with the app closed; one undo step
        """
        if kind is not _UNSET:
            _main(params, "kind", kind)
        return self._conn.call("tile.addAction", params)

    def action(self, tile=_UNSET, /, **params):
        """Change a tile's action.

        {tile, kind? (play|fade|stop|pause|play_on), targets? ([tile | "set:Name", each with
        @seconds for a group's offset]), fade? (seconds), clear? (make it an empty tile
        again)} only for tiles without audio

        `tile.action`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.action", params)

    def normalize(self, tile=_UNSET, /, **params):
        """Normalise.

        {tile? (default: every tile in the set), set?, target? (loudness|peak; default
        loudness), level? (default −16 LUFS or −1 dBFS)} sets gainDb so the trimmed clip
        reaches the level → {target, tiles: [{tile, title, measured, gain_db}], skipped}

        `tile.normalize`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.normalize", params)

    def picture(self, path=_UNSET, /, **params):
        """Choose Picture.

        {tile, path? (an image; without it the app opens a file picker)}

        `tile.picture`; changes the library; works with the app closed; one undo step
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("tile.picture", params)

    def picture_clear(self, tile=_UNSET, /, **params):
        """Remove Picture.

        {tile}

        `tile.pictureClear`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.pictureClear", params)

    def send(self, tile=_UNSET, /, **params):
        """Also send to an output.

        {tile, output (an output's name, or main), gainDb? (-24 to +24, on top of the tile's
        level; default 0)} adds the send, or changes its level; the tile plays there too
        from its next start → the tile

        `tile.send`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.send", params)

    def unsend(self, tile=_UNSET, /, **params):
        """Stop sending to an output.

        {tile, output} a tile with no sends left plays on Main

        `tile.unsend`; changes the library; works with the app closed; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("tile.unsend", params)


class _Set(_Area):
    """The `set.*` commands."""

    def transcribe(self, set=_UNSET, /, **params):
        """Transcribe Tiles Without Subtitles.

        {set? (default current), engine?, lang?, gaps? (default false: also the tiles with
        subtitles from elsewhere, for the lines heard between theirs), ask? (the window asks
        first, naming the engine, where the audio is sent and, with a box to tick, how many
        tiles have subtitles whose gaps it could fill; then → {asked, set, tiles,
        subtitled, engine, sendsTo})} tile.transcribe for every tile in the set that has a
        sound and no subtitles (with gaps, and every one whose subtitles aren't
        machine-made) → as tile.transcribe

        `set.transcribe`; changes the library; works with the app closed
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("set.transcribe", params)

    def list(self, **params):
        """List sets.

        → [{id, name, tiles, layout, grid (Settings > Grid layout), columns, rows (tiles
        per grid row, top to bottom), auto_advance, current}]

        `set.list`; read-only
        """
        return self._conn.call("set.list", params)

    def create(self, name=_UNSET, /, **params):
        """New Set.

        {name? (default "Set N"; the app then lets you type one)} → {id}

        `set.create`; changes the library; works with the app closed; one undo step
        """
        if name is not _UNSET:
            _main(params, "name", name)
        return self._conn.call("set.create", params)

    def rename(self, name=_UNSET, /, **params):
        """Rename Set.

        {name? (without it the app edits the name in place), set? (default current)}

        `set.rename`; changes the library; works with the app closed; one undo step
        """
        if name is not _UNSET:
            _main(params, "name", name)
        return self._conn.call("set.rename", params)

    def duplicate(self, set=_UNSET, /, **params):
        """Duplicate Set.

        {set?} → {id}

        `set.duplicate`; changes the library; works with the app closed; one undo step
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("set.duplicate", params)

    def delete(self, set=_UNSET, /, **params):
        """Delete Set.

        {set?} the last set can't be deleted

        `set.delete`; changes the library; works with the app closed; one undo step; destructive: refused under safe=True
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("set.delete", params)

    def select(self, set=_UNSET, /, **params):
        """Select a set.

        {set}

        `set.select`; changes the library; works with the app closed
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("set.select", params)

    def next(self, **params):
        """Next Set.

        {} the set below the current one in the sidebar (stops at the last) → {id, name}

        `set.next`; changes the library; works with the app closed
        """
        return self._conn.call("set.next", params)

    def prev(self, **params):
        """Previous Set.

        {} the set above the current one (stops at the first) → {id, name}

        `set.prev`; changes the library; works with the app closed
        """
        return self._conn.call("set.prev", params)

    def layout(self, layout=_UNSET, /, **params):
        """Set the layout.

        {layout: grid|list, set?}

        `set.layout`; changes the library; works with the app closed
        """
        if layout is not _UNSET:
            _main(params, "layout", layout)
        return self._conn.call("set.layout", params)

    def columns(self, columns=_UNSET, /, **params):
        """Set the grid columns.

        {columns (1-12), set?} Fixed columns: the column count; Rows: the row size (how wide
        tiles get, and how many new sounds fill a row)

        `set.columns`; changes the library; works with the app closed
        """
        if columns is not _UNSET:
            _main(params, "columns", columns)
        return self._conn.call("set.columns", params)

    def rows(self, perRow=_UNSET, /, **params):
        """Even out rows.

        {perRow (1-12) | rows ([3, 8, …] or "3,8": tiles per row, top to bottom; tiles
        left over fill rows of the set's columns), set?} perRow evens every row out and sets
        columns; both clear the blank slots → {rows}

        `set.rows`; changes the library; works with the app closed; one undo step
        """
        if perRow is not _UNSET:
            _main(params, "perRow", perRow)
        return self._conn.call("set.rows", params)

    def add_blank(self, row=_UNSET, /, **params):
        """Insert a blank slot.

        {row, slot (1-based: the blank goes before this slot; slots + 1 for the row's end),
        count? (default 1), set?} an empty slot in a grid row, which stays put as tiles come
        and go around it; reading order skips it → {slots: ["xx.x", …] (x a tile, . a
        blank)}

        `set.addBlank`; changes the library; works with the app closed; one undo step
        """
        if row is not _UNSET:
            _main(params, "row", row)
        return self._conn.call("set.addBlank", params)

    def remove_blank(self, row=_UNSET, /, **params):
        """Remove a blank slot.

        {row, slot (1-based), set?} the slots after it move up one → {slots}

        `set.removeBlank`; changes the library; works with the app closed; one undo step
        """
        if row is not _UNSET:
            _main(params, "row", row)
        return self._conn.call("set.removeBlank", params)

    def list_mode(self, mode=_UNSET, /, **params):
        """List Playback Mode.

        {mode: continuous|queued, set?} Continuous: a cue that ends on its own starts the
        next armed one; Queued: it waits for GO / Enter → {mode}

        `set.listMode`; changes the library; works with the app closed
        """
        if mode is not _UNSET:
            _main(params, "mode", mode)
        return self._conn.call("set.listMode", params)

    def crossfade(self, seconds=_UNSET, /, **params):
        """List Cross-Fade.

        {seconds (0–5), set?} Continuous: how long the next cue overlaps the end of the
        one before, one fading out as the other fades in (at most half of either) →
        {seconds}

        `set.crossfade`; changes the library; works with the app closed
        """
        if seconds is not _UNSET:
            _main(params, "seconds", seconds)
        return self._conn.call("set.crossfade", params)

    def auto_advance(self, on=_UNSET, /, **params):
        """Auto-advance cues.

        {on? (default: toggle), set?} the same switch as set.listMode (on: continuous)

        `set.autoAdvance`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("set.autoAdvance", params)

    def notes(self, notes=_UNSET, /, **params):
        """Set notes.

        {notes (text; empty clears), set?} shown above the cue list

        `set.notes`; changes the library; works with the app closed; one undo step
        """
        if notes is not _UNSET:
            _main(params, "notes", notes)
        return self._conn.call("set.notes", params)

    def lock(self, set=_UNSET, /, **params):
        """Lock Set.

        {set? (default current), on? (default true)} a locked set can't be changed: its
        tiles, their order and its settings stay as they are (every change is refused with
        code locked) and its tiles still play → {set, locked}

        `set.lock`; changes the library; works with the app closed; one undo step
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("set.lock", params)

    def unlock(self, set=_UNSET, /, **params):
        """Unlock Set.

        {set? (default current)} → {set, locked}

        `set.unlock`; changes the library; works with the app closed; one undo step
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("set.unlock", params)

    def reassign_keys(self, set=_UNSET, /, **params):
        """Reassign hotkeys by row.

        {set?} drops the keys set by hand: every pad takes its slot's key again (row 1: 1–=,
        row 2: q–], row 3: a–', row 4: z–/, rows 5–8: the same with alt+)

        `set.reassignKeys`; changes the library; works with the app closed; one undo step
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("set.reassignKeys", params)

    def import_(self, path=_UNSET, /, **params):
        """Import Set.

        {path? (a .lsset set file, a .zip, a pack folder, or a Farrago export: a .farragoset
        folder or its zip; without it the app opens a file picker), ask? (the window asks
        first, as for a set file opened from the file manager; then → {asked, path}),
        raise? (with ask: bring the window to the front), useTheme? (a set file carrying a
        theme: true uses it now, false doesn't; without it the window asks)} adds its sets
        after yours, and installs the theme it carries as a user theme, replacing one of the
        same name → {sets: [{id, name, tiles}], theme (null, or {id, name, base, path,
        replaced, used, asked, warnings}, or {error}), farrago? (a Farrago export:
        {imported: [title], skipped: [{title, reason}], dropped: [setting (n tiles)],
        warnings})}; missing or unreadable sounds are skipped, not fatal

        `set.import`; changes the library; works with the app closed; one undo step
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("set.import", params)

    def export(self, path=_UNSET, /, **params):
        """Export Set.

        {path? (a .lsset file to write; without it the app asks where), set? (default
        current), all? (every set)} clips, pictures and every setting in one file, and the
        user theme in use → {path, sets, tiles, bytes, skipped, theme (its id, or null)}

        `set.export`; changes the library; works with the app closed
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("set.export", params)

    def render(self, folder=_UNSET, /, **params):
        """Export Set Audio.

        {folder? (made if missing; without it the app opens the Export Audio window), set?
        (default current), format?, rate?} every tile with audio, as "NN Title.wav" in set
        order, never over an existing file → {job, format, files, skipped}

        `set.render`; read-only
        """
        if folder is not _UNSET:
            _main(params, "folder", folder)
        return self._conn.call("set.render", params)


class _Transcribe(_Area):
    """The `transcribe.*` commands."""

    def status(self, job=_UNSET, /, **params):
        """Show transcriptions.

        {job? (default every one)} → {jobs: [{job, total, done, finished, cancelled,
        current ({tile, title, engine, fraction (0-1, or null while unknown)} or null),
        queued, results: [{tile, title, engine, model, lang, heard: {lines, words},
        replaced, lines, kept_aligned, aligned: {lines, matched, placed, missed}, refined:
        {words, moved, mean_ms, max_ms}, changed}], errors: [{tile, title, error}]}], busy}

        `transcribe.status`; read-only
        """
        if job is not _UNSET:
            _main(params, "job", job)
        return self._conn.call("transcribe.status", params)

    def cancel(self, job=_UNSET, /, **params):
        """Cancel Transcription.

        {job? (default every one)} stops the tile being transcribed and drops the ones
        waiting; nothing is changed → {cancelled (how many tiles)}

        `transcribe.cancel`; changes the library; works with the app closed
        """
        if job is not _UNSET:
            _main(params, "job", job)
        return self._conn.call("transcribe.cancel", params)

    def engines(self, **params):
        """List transcription engines.

        → {setting (auto or an engine), uses (the engine tile.transcribe would use, or
        null), sendsTo (where it sends the audio, or null), reason (why none can run, with
        what to install), engines: [{engine, label, available, reason, servers? (quip:
        [{server, name, transcribes, reason, upload: {maxBytes, maxSeconds}, used, why}])}],
        apple ({permission: not-asked|denied|restricted|allowed (null outside the app),
        locale, language, onDevice, canAsk} on macOS and iPadOS, else null; asking it never
        prompts)} in auto's order: Apple Speech, whisper-cli, QuipClipper (only the ones
        this version has). Asks each connected QuipClipper whether it transcribes (GET
        /api/config) if it hasn't been asked yet

        `transcribe.engines`; read-only
        """
        return self._conn.call("transcribe.engines", params)

    def allow_speech(self, **params):
        """Allow Speech Recognition.

        {} macOS and iPadOS, in the app only: shows the system's Speech Recognition
        permission prompt if it hasn't been answered (the only thing that ever shows it,
        D-288), else changes nothing. Recognition stays on the device → {asked, permission
        (as it was), apple (as transcribe.engines), reason (why Apple Speech still can't
        run, or null)}

        `transcribe.allowSpeech`; needs the running app
        """
        return self._conn.call("transcribe.allowSpeech", params)


class _Transport(_Area):
    """The `transport.*` commands."""

    def stop_all(self, immediate=_UNSET, /, **params):
        """Stop All.

        {immediate? (default false), fade? (seconds; default the Stop All fade setting)}
        without either, a Stop All while the last one is still fading stops at once (Esc's
        second press); at once is a 0.1 s de-click → {fade, second}

        `transport.stopAll`; needs the running app
        """
        if immediate is not _UNSET:
            _main(params, "immediate", immediate)
        return self._conn.call("transport.stopAll", params)

    def pause_all(self, **params):
        """Pause All.

        pause everything playing, pausable or not; with nothing playing, resume what's
        paused → {paused, resumed}

        `transport.pauseAll`; needs the running app
        """
        return self._conn.call("transport.pauseAll", params)

    def fade_all(self, fade=_UNSET, /, **params):
        """Fade All.

        {fade? (seconds, default 2)} fade everything out and stop it → {fade, second}

        `transport.fadeAll`; needs the running app
        """
        if fade is not _UNSET:
            _main(params, "fade", fade)
        return self._conn.call("transport.fadeAll", params)


class _Cue(_Area):
    """The `cue.*` commands."""

    def go(self, set=_UNSET, /, **params):
        """Play Next Cue.

        {set? (default current), tile? (select this cue first)} in a List set, Play
        Selection (Enter): play the selected cue and stop the rest of the list (over the
        cross-fade in Continuous); again while it plays, stop it (pause or resume a Pausable
        cue); a vamping cue plays on (D-155). In a grid set, the GO button: play the cue
        under the pointer and move it on; while a tile loops its vamp, let it play on
        instead → {tile, title, position, action?, played_on?}

        `cue.go`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("cue.go", params)

    def set(self, index=_UNSET, /, **params):
        """Set the next cue.

        {index (1-based position) | tile (in its own set), set?} moves the pointer there
        without playing → {set, next}

        `cue.set`; needs the running app
        """
        if index is not _UNSET:
            _main(params, "index", index)
        return self._conn.call("cue.set", params)

    def top(self, set=_UNSET, /, **params):
        """Back to Top of Cue List.

        {set?}

        `cue.top`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("cue.top", params)

    def next(self, set=_UNSET, /, **params):
        """Skip to Next Cue.

        {set?} move the pointer past the next cue without playing it → {set, next, title}

        `cue.next`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("cue.next", params)

    def prev(self, set=_UNSET, /, **params):
        """Back to Previous Cue.

        {set?} move the pointer back one cue without playing → {set, next, title}

        `cue.prev`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("cue.prev", params)

    def reset(self, set=_UNSET, /, **params):
        """Reset List.

        {set?} arm every cue again and make the first one next; nothing stops → {set,
        next, title}

        `cue.reset`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("cue.reset", params)

    def arm(self, tile=_UNSET, /, **params):
        """Arm or Disarm a Cue.

        {tile, armed? (default: toggle)} a List row's armed marker: an armed cue is played
        when the list reaches it, a played one is passed over → {tile, title, armed}

        `cue.arm`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("cue.arm", params)


class _Edit(_Area):
    """The `edit.*` commands."""

    def undo(self, **params):
        """Undo.

        {} the last change to sets or tiles (⌘/Ctrl+Z outside text fields) → {undid}

        `edit.undo`; needs the running app
        """
        return self._conn.call("edit.undo", params)

    def redo(self, **params):
        """Redo.

        {} the last undone change (⌘/Ctrl+Shift+Z) → {redid}

        `edit.redo`; needs the running app
        """
        return self._conn.call("edit.redo", params)


class _Bind(_Area):
    """The `bind.*` commands."""

    def list(self, **params):
        """List bindings.

        → [{id, source, what, command, params, value, enabled}] pads, keys and OSC
        addresses mapped to commands

        `bind.list`; read-only
        """
        return self._conn.call("bind.list", params)

    def add(self, source=_UNSET, /, **params):
        """Add a binding.

        {source (midi:note:36[:ch10][:port], midi:cc:7, midi:program:2, key:Ctrl+Alt+1,
        osc:/address, or an object), command? (default tile.trigger), tile?, params?
        (object), value? (the parameter a fader fills, e.g. volume)} → the binding

        `bind.add`; changes the library; works with the app closed
        """
        if source is not _UNSET:
            _main(params, "source", source)
        return self._conn.call("bind.add", params)

    def remove(self, id=_UNSET, /, **params):
        """Remove bindings.

        {id | tile | source | all: true} → {removed}

        `bind.remove`; changes the library; works with the app closed
        """
        if id is not _UNSET:
            _main(params, "id", id)
        return self._conn.call("bind.remove", params)

    def enable(self, id=_UNSET, /, **params):
        """Turn a binding on or off.

        {id, on? (default: toggle)}

        `bind.enable`; changes the library; works with the app closed
        """
        if id is not _UNSET:
            _main(params, "id", id)
        return self._conn.call("bind.enable", params)

    def learn(self, tile=_UNSET, /, **params):
        """Learn a MIDI binding.

        {tile?, command? (default tile.trigger), params?, value?, from? (midi|key; default
        midi), cancel?} the next pad or control on a MIDI controller, or the next key
        combination pressed in the window (within 30 s), becomes the binding; a fader
        learned for master.volume or tile.set fills volume

        `bind.learn`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("bind.learn", params)


class _Hotkey(_Area):
    """The `hotkey.*` commands."""

    def status(self, **params):
        """Show system-wide keys.

        → {available, note, active, failed} key bindings work from any app on macOS and in
        X11 sessions; on Wayland use the compositor's binds

        `hotkey.status`; needs the running app
        """
        return self._conn.call("hotkey.status", params)


class _Osc(_Area):
    """The `osc.*` commands."""

    def enable(self, on=_UNSET, /, **params):
        """Turn OSC on or off.

        {on? (default true), port? (default 9000), lan? (listen to other machines too;
        default false: this machine only), feedback? (host:port to send state to, or null;
        see OSC above), levels? (send each playing tile's level with the feedback, ten times
        a second; default true)} OSC has no password: keep lan off unless the network is
        yours

        `osc.enable`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("osc.enable", params)

    def status(self, **params):
        """Show OSC.

        → {enabled, listening, error, received, lan, feedback, levels}

        `osc.status`; needs the running app
        """
        return self._conn.call("osc.status", params)


class _Midi(_Area):
    """The `midi.*` commands."""

    def ports(self, **params):
        """List MIDI inputs.

        → {enabled, ports, connected, error, auto_map, bank, colors}

        `midi.ports`; needs the running app
        """
        return self._conn.call("midi.ports", params)

    def enable(self, on=_UNSET, /, **params):
        """Turn MIDI input on or off.

        {on? (default true), ports? (names or parts of names; null = every port)}

        `midi.enable`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("midi.enable", params)

    def feedback(self, on=_UNSET, /, **params):
        """Light MIDI pads.

        {on? (default true), lit? (velocity while the tile plays, 0-127; default 127),
        unlit? (default 0), channel? (for auto-mapped pads, 1-16), colors? (pads in their
        tile's colour: launchpad, apc-mini, apc-mini-mk2, a JSON table {idle: {red: 5, ...},
        playing?, idle_channel?, playing_channel?, empty?}, or off)} pads bound to tiles
        light up while their tile plays (on the controller's matching output)

        `midi.feedback`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("midi.feedback", params)

    def auto_map(self, base=_UNSET, /, **params):
        """Map MIDI notes to tiles.

        {base (the note that plays tile #1, e.g. 36; null or off to stop), bank? (pads per
        page for midi.page; 0 = no pages)} notes without a binding of their own play the
        current set's tiles by position

        `midi.autoMap`; changes the library; works with the app closed
        """
        if base is not _UNSET:
            _main(params, "base", base)
        return self._conn.call("midi.autoMap", params)

    def page(self, page=_UNSET, /, **params):
        """Pad Page.

        {page? (from 1) | delta? (+1 or -1)} which tiles auto-mapped pads play, a bank at a
        time → {page, pages, bank}

        `midi.page`; needs the running app
        """
        if page is not _UNSET:
            _main(params, "page", page)
        return self._conn.call("midi.page", params)

    def page_up(self, **params):
        """Next Pad Page.

        {} auto-mapped pads play the next bank of tiles → {page, pages, bank}

        `midi.pageUp`; needs the running app
        """
        return self._conn.call("midi.pageUp", params)

    def page_down(self, **params):
        """Previous Pad Page.

        {} → {page, pages, bank}

        `midi.pageDown`; needs the running app
        """
        return self._conn.call("midi.pageDown", params)


class _Master(_Area):
    """The `master.*` commands."""

    def volume(self, volume=_UNSET, /, **params):
        """Set the master volume.

        {volume (0-1) | db (≤ 0)}

        `master.volume`; changes the library; works with the app closed
        """
        if volume is not _UNSET:
            _main(params, "volume", volume)
        return self._conn.call("master.volume", params)

    def up(self, **params):
        """Master Volume Up.

        {} +3 dB

        `master.up`; changes the library; works with the app closed
        """
        return self._conn.call("master.up", params)

    def down(self, **params):
        """Master Volume Down.

        {} −3 dB; below −60 dB goes silent

        `master.down`; changes the library; works with the app closed
        """
        return self._conn.call("master.down", params)


class _Record(_Area):
    """The `record.*` commands."""

    def start(self, tile=_UNSET, /, **params):
        """Record New Tile.

        {tile? (record into this tile, replacing its audio), set?, title?} records from the
        input (Settings ▸ Voice chooses it) into a new tile at the end of the set, or into
        the tile → {tile, title, recording, waiting (the input is still opening: the
        recording starts when it does)}

        `record.start`; needs the running app; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("record.start", params)

    def stop(self, **params):
        """Stop Recording.

        {} the recording becomes the tile's audio (a WAV in the library), playable at once →
        {tile, title, seconds}

        `record.stop`; needs the running app; one undo step
        """
        return self._conn.call("record.stop", params)

    def toggle(self, tile=_UNSET, /, **params):
        """Record / Stop.

        {tile?, set?, title?} record.start, or record.stop while recording: one pad or key
        for both

        `record.toggle`; needs the running app; one undo step
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("record.toggle", params)

    def status(self, **params):
        """Show the recording.

        → {recording, tile?, title?, seconds?}

        `record.status`; read-only
        """
        return self._conn.call("record.status", params)


class _Replay(_Area):
    """The `replay.*` commands."""

    def enable(self, seconds=_UNSET, /, **params):
        """Instant replay.

        {seconds? (5-300, default 30; 0 or off turns it off)} keep the last seconds of the
        input, ready for replay.save → {seconds, error}

        `replay.enable`; changes the library; works with the app closed
        """
        if seconds is not _UNSET:
            _main(params, "seconds", seconds)
        return self._conn.call("replay.enable", params)

    def save(self, seconds=_UNSET, /, **params):
        """Save Instant Replay.

        {seconds? (default: all that's kept), title?, set?} the last seconds of the input as
        a new tile, playable at once → {tile, title, seconds}

        `replay.save`; needs the running app; one undo step
        """
        if seconds is not _UNSET:
            _main(params, "seconds", seconds)
        return self._conn.call("replay.save", params)


class _Duck(_Area):
    """The `duck.*` commands."""

    def voice(self, on=_UNSET, /, **params):
        """Duck under my voice.

        {on? (default true), threshold? (dBFS, -80 to 0; default -40)} beds dip while the
        microphone is above the threshold → {on, threshold, error, waiting}

        `duck.voice`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("duck.voice", params)

    def hold(self, on=_UNSET, /, **params):
        """Duck Beds (Talk-Over).

        {on? (default: toggle)} hold every ducked tile down, as a press/release pair from a
        key bind or pad → {on}

        `duck.hold`; needs the running app
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("duck.hold", params)


class _Input(_Area):
    """The `input.*` commands."""

    def list(self, **params):
        """List inputs.

        → {devices, default (the system default input's name), current, level_db, error,
        waiting (the input is still opening; on macOS, maybe asking for the microphone)}

        `input.list`; needs the running app
        """
        return self._conn.call("input.list", params)


class _Meter(_Area):
    """The `meter.*` commands."""

    def clear_clip(self, **params):
        """Clear the clip light.

        {}

        `meter.clearClip`; needs the running app
        """
        return self._conn.call("meter.clearClip", params)


class _Render(_Area):
    """The `render.*` commands."""

    def status(self, job=_UNSET, /, **params):
        """Show audio exports.

        {job? (default every one)} → {renders: [{job, format, files, done, total,
        fraction, current, finished, cancelled, written, errors}]}

        `render.status`; read-only
        """
        if job is not _UNSET:
            _main(params, "job", job)
        return self._conn.call("render.status", params)

    def cancel(self, job=_UNSET, /, **params):
        """Cancel Export.

        {job? (default every one under way)} stops rendering; a half-written file is removed
        → {cancelled}

        `render.cancel`; needs the running app
        """
        if job is not _UNSET:
            _main(params, "job", job)
        return self._conn.call("render.cancel", params)


class _Import(_Area):
    """The `import.*` commands."""

    def cancel(self, **params):
        """Cancel Import.

        {} stop the copies and ffmpeg conversions under way; their tiles stay, marked
        cancelled → {cancelled}

        `import.cancel`; needs the running app
        """
        return self._conn.call("import.cancel", params)


class _View(_Area):
    """The `view.*` commands."""

    def sidebar(self, open=_UNSET, /, **params):
        """Setlist.

        {open? (default: toggle)} show or hide the setlist (the sets sidebar) → {open}

        `view.sidebar`; changes the library; works with the app closed
        """
        if open is not _UNSET:
            _main(params, "open", open)
        return self._conn.call("view.sidebar", params)

    def inspector(self, open=_UNSET, /, **params):
        """Inspector.

        {open? (default: toggle, or true with tab), tab? (play|sound|cue|control: the tile
        inspector's tab)} show or hide the inspector: the selected tiles' settings, or with
        none selected the current set's → {open, tab}

        `view.inspector`; changes the library; works with the app closed
        """
        if open is not _UNSET:
            _main(params, "open", open)
        return self._conn.call("view.inspector", params)

    def perform(self, **params):
        """Perform Mode (deprecated).

        {} deprecated (D-147): there are no modes; hides the inspector and deselects →
        {open}

        `view.perform`; needs the running app
        """
        return self._conn.call("view.perform", params)

    def edit(self, **params):
        """Edit Mode (deprecated).

        {} deprecated (D-147): shows the inspector → {open}

        `view.edit`; needs the running app
        """
        return self._conn.call("view.edit", params)

    def toggle_edit(self, **params):
        """Toggle Edit Mode (deprecated).

        {} deprecated (D-147): view.inspector → {open}

        `view.toggleEdit`; needs the running app
        """
        return self._conn.call("view.toggleEdit", params)

    def lock_edit(self, on=_UNSET, /, **params):
        """Lock Edit Mode (deprecated).

        {on? (default: toggle)} deprecated (D-147): locks or unlocks the current set
        (set.lock) → {set, locked}

        `view.lockEdit`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("view.lockEdit", params)

    def select(self, tile=_UNSET, /, **params):
        """Select a tile.

        {tile? (none closes the inspector), add? (⌘-click: add it to the selection, or
        take it out), remove? (Alt-click: take it out of the selection; an unselected tile
        leaves the selection as it is), range? (Shift-click: every tile in the rectangle of
        slots, rows by columns, between the selected one and it, in its set; with add,
        ⌘-Shift-click: add the rectangle to the selection), columns? (with range: lay the
        set out in lines of this many instead of its rows, as Auto/Fixed columns show it; 1
        = the run in set order), tiles? ([tile]: select these)} several selected tiles edit
        together in the inspector → {selected}

        `view.select`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("view.select", params)

    def search(self, query=_UNSET, /, **params):
        """Find.

        {query? (the search box's text; the board dims the tiles it doesn't find), clear?
        (empty the box; close is the old name), go? (go to hit #index: its set, the tile
        selected, its pad focused; the box empties), play? (play hit #index, or the first
        that can play; the box keeps its text), index? (from 1, default 1)} without
        parameters it puts the keyboard in the search box → {open, query, hits: [{tile,
        title, set, position, matched: [{field, text}]}], went?, played?}

        `view.search`; needs the running app
        """
        if query is not _UNSET:
            _main(params, "query", query)
        return self._conn.call("view.search", params)

    def editor(self, tile=_UNSET, /, **params):
        """Edit Waveform.

        {tile? (default: the selected tile), open? (default: true with tile, else toggle)} a
        full-window editor for the tile's trim, vamp and markers, in place of the board;
        selecting another tile closes it → {open, tile}

        `view.editor`; needs the running app
        """
        if tile is not _UNSET:
            _main(params, "tile", tile)
        return self._conn.call("view.editor", params)

    def editor_word(self, **params):
        """Select a Subtitle Word.

        {line? (with word), word? (from 1), grip? (start|end|word: what the arrow keys move;
        default start, or the grip already chosen; given alone, the selected word's), clear?
        (true: select no word)} the word selected in the waveform editor's subtitle lane;
        with nothing, it reports → {tile, line, word, grip, text, start, end} | {tile,
        selected: null}

        `view.editorWord`; needs the running app
        """
        return self._conn.call("view.editorWord", params)

    def editor_select(self, **params):
        """Select in the Editor.

        {from, to (seconds into the file) | clear: true} the waveform editor's selection
        (dragging across the waveform), which Trim to Selection (tile.set
        trimStart/trimEnd), Vamp Selection (vampStart/vampEnd), Play Selection (tile.preview
        from/to) and view.editorZoom selection act on; with neither, it reports it; closing
        the editor clears it → {tile, selection: {from, to} | null}

        `view.editorSelect`; needs the running app
        """
        return self._conn.call("view.editorSelect", params)

    def editor_zoom(self, **params):
        """Zoom the Editor.

        {fit: true | trim: true | selection: true | from, to (seconds into the file) | by (2
        zooms in, 0.5 out, around the cursor or the middle)} what the waveform editor shows;
        trim and selection leave a little room either side → {tile, fit | from, to | by}

        `view.editorZoom`; needs the running app
        """
        return self._conn.call("view.editorZoom", params)

    def waveforms(self, on=_UNSET, /, **params):
        """Show Waveforms.

        {on? (default: toggle)}

        `view.waveforms`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("view.waveforms", params)

    def meters(self, on=_UNSET, /, **params):
        """Show Level Meters.

        {on? (default: toggle)}

        `view.meters`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("view.meters", params)

    def theme(self, theme=_UNSET, /, **params):
        """Theme.

        {theme: console|panel} the built-in themes; theme.use picks any theme

        `view.theme`; changes the library; works with the app closed
        """
        if theme is not _UNSET:
            _main(params, "theme", theme)
        return self._conn.call("view.theme", params)

    def appearance(self, appearance=_UNSET, /, **params):
        """Appearance.

        {appearance: dark|light|system|high_contrast|standard} dark, light and system are
        the Console theme's; high_contrast applies to both themes; standard is the theme's
        own look (after high_contrast, dark; otherwise no change)

        `view.appearance`; changes the library; works with the app closed
        """
        if appearance is not _UNSET:
            _main(params, "appearance", appearance)
        return self._conn.call("view.appearance", params)

    def zoom(self, zoom=_UNSET, /, **params):
        """Zoom.

        {zoom: in|out|reset or a percent (50–300)} in and out step through 50, 67, 75, 80,
        90, 100, 110, 125, 150, 175, 200, 250, 300 → {zoom}

        `view.zoom`; changes the library; works with the app closed
        """
        if zoom is not _UNSET:
            _main(params, "zoom", zoom)
        return self._conn.call("view.zoom", params)

    def settings(self, open=_UNSET, /, **params):
        """Settings.

        {open? (default true)}

        `view.settings`; needs the running app
        """
        if open is not _UNSET:
            _main(params, "open", open)
        return self._conn.call("view.settings", params)

    def shortcuts(self, **params):
        """Keyboard Shortcuts.

        {}

        `view.shortcuts`; needs the running app
        """
        return self._conn.call("view.shortcuts", params)

    def quip(self, open=_UNSET, /, **params):
        """Browse QuipClipper.

        {open? (default: toggle, or open when anything else is given), server? (a connected
        one), folder?, query?, new? (true: list the clips new since last time, on server or
        the first with any, and count them as seen)} the QuipClipper window: a connected
        server's clips library, to preview and add from (quip.browse, quip.preview,
        quip.add) → {open, server?, new?}

        `view.quip`; needs the running app
        """
        if open is not _UNSET:
            _main(params, "open", open)
        return self._conn.call("view.quip", params)

    def about(self, **params):
        """About.

        {}

        `view.about`; needs the running app
        """
        return self._conn.call("view.about", params)

    def screenshot(self, path=_UNSET, /, **params):
        """Save a screenshot.

        {path (a .png), window? (a set open in a window of its own: that window instead of
        the main one)} the window once it has settled; the file appears a moment later (up
        to a second when the window is hidden)

        `view.screenshot`; needs the running app
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("view.screenshot", params)


class _Theme(_Area):
    """The `theme.*` commands."""

    def list(self, **params):
        """List themes.

        → {themes: [{id, name, base (console|panel), builtin, path (an installed theme's
        folder; edit its files there and the window reloads them), current}], current,
        warnings (where the theme in use is below WCAG AA)}

        `theme.list`; needs the running app
        """
        return self._conn.call("theme.list", params)

    def use(self, theme=_UNSET, /, **params):
        """Use a theme.

        {theme (an id or name from theme.list; console and panel are the built-ins)} View ▸
        Theme lists the installed ones too → {theme, name, base, warnings}

        `theme.use`; changes the library; works with the app closed
        """
        if theme is not _UNSET:
            _main(params, "theme", theme)
        return self._conn.call("theme.use", params)

    def import_(self, path=_UNSET, /, **params):
        """Import Theme.

        {path? (a .lstheme folder or zip, or its theme.json; without it the app opens a file
        picker), use? (default true)} copies it into the data folder's themes, replacing one
        with the same name; a theme below WCAG AA is still imported, with warnings →
        {theme: {id, name, base, path}, replaced, used, warnings}

        `theme.import`; changes the library; works with the app closed
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("theme.import", params)

    def export(self, path=_UNSET, /, **params):
        """Export Theme.

        {path? (ending .lstheme or .zip for a zip, else a folder; without it the app asks
        where), theme? (default: the one in use)} a built-in is written as theme.json and
        tokens.json, a FigCraft variables export to start a theme from → {path}

        `theme.export`; needs the running app
        """
        if path is not _UNSET:
            _main(params, "path", path)
        return self._conn.call("theme.export", params)

    def remove(self, theme=_UNSET, /, **params):
        """Remove a theme.

        {theme} an installed theme; the built-in it was based on takes over if it was in use
        → {removed, current}

        `theme.remove`; changes the library; works with the app closed
        """
        if theme is not _UNSET:
            _main(params, "theme", theme)
        return self._conn.call("theme.remove", params)


class _App(_Area):
    """The `app.*` commands."""

    def check_for_updates(self, **params):
        """Check for Updates.

        {again? (default true: ask GitHub now; false: only the last answer), show? (default
        true: the window shows the answer, as the menu item does)} asks GitHub for the
        project's latest published release (drafts and pre-releases never count) and
        compares it with this build; nothing is downloaded. Returns at once with {checking:
        true}; the CLI waits for the answer → {checking, current, status:
        newer|latest|failed, latest?, url? (the release page), notes?, error?}

        `app.checkForUpdates`; read-only
        """
        return self._conn.call("app.checkForUpdates", params)

    def quit(self, **params):
        """Quit.

        {} saves first

        `app.quit`; needs the running app
        """
        return self._conn.call("app.quit", params)


class _Window(_Area):
    """The `window.*` commands."""

    def open(self, set=_UNSET, /, **params):
        """Open Set in New Window.

        {set? (default current), x?, y? (where its top-left corner goes, in the desktop's
        points), width?, height? (its inside)} the set's board in a window of its own, to
        put on another monitor or workspace; one already open is raised. It stays open
        across launches, where it was (on Wayland the compositor places it) → {set, name,
        opened (false: it was open already)}

        `window.open`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("window.open", params)

    def close(self, set=_UNSET, /, **params):
        """Close Set Window.

        {set? (default current) | all: true} the main window stays → {closed: [name]}

        `window.close`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("window.close", params)

    def list(self, **params):
        """List windows.

        → {windows: [{window (main, or the set's id), set?, name, title, x?, y? (absent
        where the compositor keeps positions to itself), width?, height?, focused}]}

        `window.list`; needs the running app
        """
        return self._conn.call("window.list", params)

    def focus(self, set=_UNSET, /, **params):
        """Bring a window to the front.

        {set? (none: the main window)} raises it and gives it the keyboard → {window}

        `window.focus`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("window.focus", params)

    def move(self, set=_UNSET, /, **params):
        """Move or resize a window.

        {set? (none: the main window), x?, y?, width?, height?} in the desktop's points;
        Wayland compositors ignore positions → {window}

        `window.move`; needs the running app
        """
        if set is not _UNSET:
            _main(params, "set", set)
        return self._conn.call("window.move", params)

    def gather(self, **params):
        """Gather Windows.

        {} every set window back beside the main window, one below another: the way back to
        a window left on a monitor that's gone → {moved}

        `window.gather`; needs the running app
        """
        return self._conn.call("window.gather", params)


class _Device(_Area):
    """The `device.*` commands."""

    def list(self, **params):
        """List output devices.

        → {devices: [name], current}

        `device.list`; needs the running app
        """
        return self._conn.call("device.list", params)

    def set(self, name=_UNSET, /, **params):
        """Choose the output device.

        {name (null = system default)}

        `device.set`; changes the library; works with the app closed
        """
        if name is not _UNSET:
            _main(params, "name", name)
        return self._conn.call("device.set", params)

    def reconnect(self, **params):
        """Reconnect the output device.

        {}

        `device.reconnect`; needs the running app
        """
        return self._conn.call("device.reconnect", params)


class _Output(_Area):
    """The `output.*` commands."""

    def list(self, **params):
        """List outputs.

        → {outputs: [{name, device, channel, open}], errors, preview: {on, device, open},
        devices}

        `output.list`; needs the running app
        """
        return self._conn.call("output.list", params)

    def add(self, name=_UNSET, /, **params):
        """Add an output.

        {name? (default "Output N"), device? (null = system default), channel? (first of its
        pair, from 1; default 1)} tiles can then be routed to it with tile.set output

        `output.add`; changes the library; works with the app closed
        """
        if name is not _UNSET:
            _main(params, "name", name)
        return self._conn.call("output.add", params)

    def set(self, name=_UNSET, /, **params):
        """Change an output.

        {name, device?, channel?, rename?}

        `output.set`; changes the library; works with the app closed
        """
        if name is not _UNSET:
            _main(params, "name", name)
        return self._conn.call("output.set", params)

    def remove(self, name=_UNSET, /, **params):
        """Remove an output.

        {name} sends to it go; a tile with none left plays on Main

        `output.remove`; changes the library; works with the app closed
        """
        if name is not _UNSET:
            _main(params, "name", name)
        return self._conn.call("output.remove", params)


class _Settings(_Area):
    """The `settings.*` commands."""

    def stop_all_fade(self, seconds=_UNSET, /, **params):
        """Set the Stop All fade.

        {seconds (0-5)} Esc's first press fades over it; 0 (the default) stops at once (a
        0.1 s de-click)

        `settings.stopAllFade`; changes the library; works with the app closed
        """
        if seconds is not _UNSET:
            _main(params, "seconds", seconds)
        return self._conn.call("settings.stopAllFade", params)

    def check_updates(self, on=_UNSET, /, **params):
        """Check for updates at launch.

        {on? (default: toggle)} off by default: then the app never contacts GitHub unless
        app.checkForUpdates is run; on, it checks once at launch and speaks up only about a
        newer release → {on}

        `settings.checkUpdates`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.checkUpdates", params)

    def stop_all_hold(self, on=_UNSET, /, **params):
        """Hold Esc for Stop All.

        {on? (default: toggle)} Esc must be held for 0.3 s before it stops everything
        (Shift+Esc and the CLI don't wait)

        `settings.stopAllHold`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.stopAllHold", params)

    def input_device(self, name=_UNSET, /, **params):
        """Choose the input (microphone).

        {name (null = system default)}

        `settings.inputDevice`; changes the library; works with the app closed
        """
        if name is not _UNSET:
            _main(params, "name", name)
        return self._conn.call("settings.inputDevice", params)

    def preview_output(self, on=_UNSET, /, **params):
        """Preview to another output.

        {on? (default true), device? (null = system default)} the inspector's Preview and
        tile.preview play there (headphones) and leave the cue pointer alone; while that
        device isn't open, previews play nowhere

        `settings.previewOutput`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.previewOutput", params)

    def duck(self, amount=_UNSET, /, **params):
        """Set the ducking.

        {amount? (dB below the bed's level, -40 to 0; default -12), attack? (s, 0-2),
        release? (s, 0-5)}

        `settings.duck`; changes the library; works with the app closed
        """
        if amount is not _UNSET:
            _main(params, "amount", amount)
        return self._conn.call("settings.duck", params)

    def grid_layout(self, mode=_UNSET, /, **params):
        """Grid Layout.

        {mode: rows|auto|columns} rows: each row holds the tiles put in it and a fuller
        row's tiles narrow (the default); auto: as many columns as show every tile without
        scrolling; columns: each set's column count, wrapping

        `settings.gridLayout`; changes the library; works with the app closed
        """
        if mode is not _UNSET:
            _main(params, "mode", mode)
        return self._conn.call("settings.gridLayout", params)

    def speak_cues(self, on=_UNSET, /, **params):
        """Speak Cue Names.

        {on? (default: toggle)} the system voice says a tile's title when it starts (macOS
        say, on the Preview output's device when Preview to another output is on; Linux
        spd-say, speech-dispatcher's own output); off by default → {on, voice, routed}

        `settings.speakCues`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.speakCues", params)

    def colored_pads(self, on=_UNSET, /, **params):
        """Coloured pads.

        {on? (default: toggle)} Console pads in their tile's colour all over; off (the
        default), a pad has a dark body with its colour on the title, the waveform and the
        border. Panel's blocks don't change → {on}

        `settings.coloredPads`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.coloredPads", params)

    def tile_keys_all_sets(self, on=_UNSET, /, **params):
        """Tile keys work in every set.

        {on? (default: toggle)} a tile's key plays it from any set: the current set's tile
        first, then the other sets in sidebar order → {on}

        `settings.tileKeysAllSets`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.tileKeysAllSets", params)

    def quip_badge(self, on=_UNSET, /, **params):
        """Show new QuipClipper clips.

        {on? (default: toggle)} the status bar shows how many clips are new on the connected
        QuipClipper servers, asking them every minute while the QuipClipper window is
        closed; on by default (status.quip_new has the counts) → {on}

        `settings.quipBadge`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.quipBadge", params)

    def quip_ask_each_import(self, on=_UNSET, /, **params):
        """Ask before each QuipClipper import.

        {on? (default: toggle)} a QuipClipper link the system opens asks "Import …?"
        before importing, even from a connected server (links from a server that isn't
        connected always ask); off by default → {on}

        `settings.quipAskEachImport`; changes the library; works with the app closed
        """
        if on is not _UNSET:
            _main(params, "on", on)
        return self._conn.call("settings.quipAskEachImport", params)

    def transcribe(self, **params):
        """Transcription settings.

        {engine? (auto|whisper|quip|apple), lang? (a language code like en; auto or "" for
        the engine's own), whisperModel? (a ggml model file for whisper-cli, e.g.
        ggml-base.en.bin; "" clears it; LEADSTINGER_WHISPER_MODEL wins), quipServer? (a
        connected QuipClipper to transcribe on; "" for the first, by address, that takes
        uploads)} the app downloads no model and needs none until you transcribe →
        {engine, lang, whisperModel, quipServer}

        `settings.transcribe`; changes the library; works with the app closed
        """
        return self._conn.call("settings.transcribe", params)

    def window_system(self, x11=_UNSET, /, **params):
        """Choose the window system (Linux).

        {x11: bool} applies at the next launch

        `settings.windowSystem`; changes the library; works with the app closed
        """
        if x11 is not _UNSET:
            _main(params, "x11", x11)
        return self._conn.call("settings.windowSystem", params)


class _Quip(_Area):
    """The `quip.*` commands."""

    def connect(self, url=_UNSET, /, **params):
        """Connect to QuipClipper.

        {url (the server's base URL, e.g. https://quip.example), token? (its API token,
        QC_API_TOKEN; - reads it from stdin; none: a server with no password, asked first
        and sent no credentials), check? (default true: try the token on the server first)}
        keeps the token in quip-servers.json in the data folder, readable only by you, never
        in the library or set files; the server's links then open → {server, checked}

        `quip.connect`; changes the library; works with the app closed
        """
        if url is not _UNSET:
            _main(params, "url", url)
        return self._conn.call("quip.connect", params)

    def disconnect(self, url=_UNSET, /, **params):
        """Disconnect from QuipClipper.

        {url} forgets the server's token (a paired one is revoked on the server too); its
        links are refused again → {server, removed, revoked}

        `quip.disconnect`; changes the library; works with the app closed
        """
        if url is not _UNSET:
            _main(params, "url", url)
        return self._conn.call("quip.disconnect", params)

    def list(self, **params):
        """List QuipClipper servers.

        → [{server, added (Unix time), token (has one), paired}] the connected servers,
        never their tokens

        `quip.list`; read-only
        """
        return self._conn.call("quip.list", params)

    def pair(self, url=_UNSET, /, **params):
        """Pair with QuipClipper.

        {url, cancel? (true: stop waiting), ask? (the window asks first, as a pair link
        does)} asks the server for a pairing and returns at once; QuipClipper shows the code
        with Allow and Deny, and on Allow gives this app a token of its own (revocable
        there). A server without pairing that has no password connects without a token →
        {server, status: starting|waiting|connected|denied|expired|cancelled|failed, code?,
        approve_url?, message?}; the CLI waits for the answer, showing the code

        `quip.pair`; changes the library; works with the app closed
        """
        if url is not _UNSET:
            _main(params, "url", url)
        return self._conn.call("quip.pair", params)

    def pairing(self, url=_UNSET, /, **params):
        """QuipClipper pairings.

        {url?} → [{server, status, code?, approve_url?, expires_in?, token?, message?}]
        the pairings started since the app started

        `quip.pairing`; read-only
        """
        if url is not _UNSET:
            _main(params, "url", url)
        return self._conn.call("quip.pairing", params)

    def discover(self, **params):
        """Find QuipClipper on the network.

        {again? (default true: look again)} listens a few seconds for QuipClipper's mDNS
        advertisement (_quipclipper._tcp; QuipClipper's QC_ADVERTISE) and checks each answer
        is QuipClipper; connected servers aren't listed → {looking, found: [{server, name,
        address, protected}], error}; the CLI waits for the look to finish

        `quip.discover`; changes the library; works with the app closed
        """
        return self._conn.call("quip.discover", params)

    def import_(self, link=_UNSET, /, **params):
        """Import from QuipClipper.

        {link (a leadstinger://quip?… link, or a clip URL
        <server>/api/clips/download/<clip>, which may end ?as=flac), title?, start?, end?
        (s: the tile's trim), set? (by name, created if missing; default current), notes?
        (the tile's notes), as? (flac: the server converts the clip to FLAC first, every
        channel kept), raise? (bring the window to the front, as opened links do), ask? (as
        an opened link: with a window, a server that isn't connected asks to connect first,
        and with settings.quipAskEachImport every link asks)} downloads the clip from a
        connected server in the background (Cancel Import stops it) into a new tile at the
        end of the set → {tile, title, set, server, clip, trimStart, trimEnd, flac}, or
        {asked, server, connected} when the window asks; a server that isn't connected is
        otherwise refused with code not_connected and nothing is fetched

        `quip.import`; changes the library; works with the app closed; one undo step
        """
        if link is not _UNSET:
            _main(params, "link", link)
        return self._conn.call("quip.import", params)

    def browse(self, folder=_UNSET, /, **params):
        """Browse QuipClipper's clips.

        {server? (default: the only one connected), folder? (in the clips library; default
        its top), query? (words every clip must have), kind? (audio|video|gif|set),
        recursive? (default false: only the folder's own clips), sort? (newest|name; default
        newest)} → {server, folder, folders, clips: [{clip, kind, title, source, episode,
        duration, channels, trim, size, modified, flac}], count, cursor}

        `quip.browse`; read-only
        """
        if folder is not _UNSET:
            _main(params, "folder", folder)
        return self._conn.call("quip.browse", params)

    def search(self, query=_UNSET, /, **params):
        """Search QuipClipper's clips.

        {query, server?, folder? (default: the whole library), kind?} every clip under the
        folder with all the words in its line, file name, source, show or episode, newest
        first → as quip.browse

        `quip.search`; read-only
        """
        if query is not _UNSET:
            _main(params, "query", query)
        return self._conn.call("quip.search", params)

    def new(self, server=_UNSET, /, **params):
        """New in QuipClipper.

        {server?, mark? (default false: true counts them as seen)} the clips added since
        they were last seen (the cursor is kept beside the token, never in the library); the
        first look at a server is the baseline and finds none → as quip.browse, plus
        {since, baseline, marked}

        `quip.new`; changes the library; works with the app closed
        """
        if server is not _UNSET:
            _main(params, "server", server)
        return self._conn.call("quip.new", params)

    def add(self, clip=_UNSET, /, **params):
        """Add from QuipClipper.

        {clip (its path in the clips library), server?, set? (by name, created if missing;
        default current), title? (default: the line)} quip.import with the clip's metadata:
        as FLAC when the server offers it, trimmed to the line when its ends are known,
        notes saying where it's from; a set file (.lsset) is imported as a set → as
        quip.import, or {set, downloading} for a set file

        `quip.add`; changes the library; works with the app closed; one undo step
        """
        if clip is not _UNSET:
            _main(params, "clip", clip)
        return self._conn.call("quip.add", params)

    def preview(self, clip=_UNSET, /, **params):
        """Preview a QuipClipper clip.

        {clip, server?, stop? (true: stop the preview), title?} fetches the clip into memory
        (never saved) and plays it on the Preview output, or on Main when Preview to is off,
        as tile.preview does (Preview to on with its device not open: refused, code
        preview_unavailable); again, or another clip, stops it first → {clip, title,
        loading} or {stopped}

        `quip.preview`; needs the running app
        """
        if clip is not _UNSET:
            _main(params, "clip", clip)
        return self._conn.call("quip.preview", params)


class LeadStinger:
    """A connection to LeadStinger (see `connect`). Use it in a `with` block, or `close()` it."""

    def __init__(self, conn):
        self._conn = conn
        #: In a `with ls.batch() as b:` block, `b.result` is the batch's reply once it ends.
        self.result = None
        self.library = _Library(conn)
        self.tile = _Tile(conn)
        self.set = _Set(conn)
        self.transcribe = _Transcribe(conn)
        self.transport = _Transport(conn)
        self.cue = _Cue(conn)
        self.edit = _Edit(conn)
        self.bind = _Bind(conn)
        self.hotkey = _Hotkey(conn)
        self.osc = _Osc(conn)
        self.midi = _Midi(conn)
        self.master = _Master(conn)
        self.record = _Record(conn)
        self.replay = _Replay(conn)
        self.duck = _Duck(conn)
        self.input = _Input(conn)
        self.meter = _Meter(conn)
        self.render = _Render(conn)
        self.import_ = _Import(conn)
        self.view = _View(conn)
        self.theme = _Theme(conn)
        self.app = _App(conn)
        self.window = _Window(conn)
        self.device = _Device(conn)
        self.output = _Output(conn)
        self.settings = _Settings(conn)
        self.quip = _Quip(conn)

    def status(self, **params):
        """Show what's playing.

        → {playing: [{tile, title, set, state, position, remaining, length, level_db (the
        tile's meter)}], master, master_db, device, sample_rate, set, cue, input_note (why
        the input is closed, or that it's still opening), input (the input recording and
        ducking use), hears_board (the input follows the board, D-127), recording,
        recording_seconds, locked (the current set), sidebar, inspector, inspector_tab,
        theme}

        `status`; read-only
        """
        return self._conn.call("status", params)

    def hello(self):
        """The app's name and version, and its pid; `offline` when it's the library, not the app."""
        return self._conn.call("hello", {})

    def call(self, command, **params):
        """Run any command by its id, e.g. `ls.call("tile.trigger", tile="Rimshot")`."""
        return self._conn.call(command, params)

    def batch(self, *, atomic=True, dry_run=False):
        """Commands written in the `with` block are sent as one `batch.run` when it ends: they
        run with nothing else acting in between, as one undo step (Script). Inside the block
        they return None; `result` holds the batch's reply afterwards (`changes` says what
        changed). A failing step raises `Error` and, unless `atomic=False`, puts the sets and
        tiles back. `dry_run=True` reports the changes and changes nothing."""
        if isinstance(self._conn, _Recorder):
            raise Error("bad_params", "a batch can't hold another batch")
        return _Batch(self._conn, atomic, dry_run)

    def close(self):
        self._conn.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()
        return False


def connect(cli=None, *, data_dir=None, safe=False, allow_dirs=(), offline=False, ssh=None):
    """Connect to the running app, or to the library when the app is closed (it is then
    locked until `close()`, and commands that need the app raise `not_running`).

    `cli`: the `leadstinger-cli` to run (default LEADSTINGER_CLI, else from the PATH).
    `data_dir`: another library. `safe`, `allow_dirs`: refuse what `--safe` refuses.
    `offline`: never use the running app. `ssh`: "user@host" whose key has the
    `leadstinger-cli --ssh` forced command (always safe; the other options are the host's).
    """
    if ssh:
        argv = ["ssh", "-T", ssh, "connect"]
    else:
        argv = [cli or os.environ.get("LEADSTINGER_CLI") or "leadstinger-cli"]
        if data_dir:
            argv += ["--data-dir", os.fspath(data_dir)]
        if safe:
            argv.append("--safe")
        for d in allow_dirs:
            argv += ["--allow-dir", os.fspath(d)]
        if offline:
            argv.append("--offline")
        argv.append("connect")
    return LeadStinger(_Process(argv))
