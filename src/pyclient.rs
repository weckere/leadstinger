//! The Python client, `clients/python/leadstinger.py`, generated from the command registry
//! (`leadstinger-cli commands --python`), so it can't drift from the commands. A test fails
//! when the checked-in file is stale.
//!
//! The client talks to `leadstinger-cli connect`, so it reaches the running app or, when
//! it's closed, the library; with `ssh=` it reaches a remote one through the `--ssh`
//! forced command. Each command is a method, grouped by area (`ls.tile.trigger(...)`),
//! with snake_case names and the registry's own parameter names as keyword arguments.

use std::fmt::Write;

use crate::commands::{by_area, Kind, Spec, COMMANDS};

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or",
    "pass", "raise", "return", "try", "while", "with", "yield",
];

/// Areas with no class of their own: `status` is a method of the client, and `batch.run`
/// is what `ls.batch()` sends.
const NO_AREA: &[&str] = &["status", "batch"];

/// A Python name for a registry word: snake_case, with `_` after a keyword.
pub fn py_name(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

fn class_name(area: &str) -> String {
    let mut c = area.chars();
    let first = c.next().map(|f| f.to_ascii_uppercase()).unwrap_or('_');
    format!("_{first}{}", c.as_str())
}

/// Words to lines of at most `width` characters, each starting with `indent`.
fn wrap(text: &str, indent: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && indent.len() + line.len() + 1 + word.chars().count() > width {
            let _ = writeln!(out, "{indent}{line}");
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        let _ = writeln!(out, "{indent}{line}");
    }
    out
}

/// Text safe inside a `"""` docstring.
fn doc_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace("\"\"\"", "\\\"\\\"\\\"")
}

fn docstring(s: &Spec, indent: &str) -> String {
    let mut d = format!("{indent}\"\"\"{}.\n\n", doc_text(s.label.trim_end_matches('…').trim_end_matches('.')));
    if !s.params.is_empty() {
        d.push_str(&wrap(&doc_text(s.params), indent, 92));
        d.push('\n');
    }
    let kind = match s.kind {
        Kind::Query => "read-only",
        Kind::Library => "changes the library; works with the app closed",
        Kind::Live => "needs the running app",
    };
    let mut tags = vec![format!("`{}`", s.id), kind.to_owned()];
    if s.undo {
        tags.push("one undo step".into());
    }
    if s.destructive {
        tags.push("destructive: refused under safe=True".into());
    }
    let _ = write!(d, "{indent}{}\n{indent}\"\"\"\n", tags.join("; "));
    d
}

fn method(out: &mut String, s: &Spec, name: &str) {
    match s.main {
        Some(main) => {
            let _ = writeln!(out, "    def {name}(self, {main}=_UNSET, /, **params):");
            out.push_str(&docstring(s, "        "));
            let _ = writeln!(out, "        if {main} is not _UNSET:\n            _main(params, {main:?}, {main})");
        }
        None => {
            let _ = writeln!(out, "    def {name}(self, **params):");
            out.push_str(&docstring(s, "        "));
        }
    }
    let _ = writeln!(out, "        return self._conn.call({:?}, params)\n", s.id);
}

/// The whole module.
pub fn generate() -> String {
    let mut out = String::from(HEAD);
    let areas: Vec<_> = by_area().into_iter().filter(|(a, _)| !NO_AREA.contains(a)).collect();
    for (area, specs) in &areas {
        let _ = writeln!(out, "\nclass {}(_Area):\n    \"\"\"The `{area}.*` commands.\"\"\"\n", class_name(area));
        for s in specs {
            method(&mut out, s, &py_name(s.id.split_once('.').map_or(s.id, |(_, m)| m)));
        }
    }
    out.push_str(CLIENT_HEAD);
    for (area, _) in &areas {
        let _ = writeln!(out, "        self.{} = {}(conn)", py_name(area), class_name(area));
    }
    out.push('\n');
    if let Some(status) = COMMANDS.iter().find(|s| s.id == "status") {
        method(&mut out, status, "status");
    }
    out.push_str(TAIL);
    out
}

const HEAD: &str = r#"""""LeadStinger from Python: every command in the app, as a method.

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
"#;

const CLIENT_HEAD: &str = r#"
class LeadStinger:
    """A connection to LeadStinger (see `connect`). Use it in a `with` block, or `close()` it."""

    def __init__(self, conn):
        self._conn = conn
        #: In a `with ls.batch() as b:` block, `b.result` is the batch's reply once it ends.
        self.result = None
"#;

const TAIL: &str = r#"    def hello(self):
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
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_python() {
        assert_eq!(py_name("subtitlesSet"), "subtitles_set");
        assert_eq!(py_name("import"), "import_");
        assert_eq!(py_name("list"), "list");
        // No two commands of an area end up with one method name.
        for (area, specs) in by_area() {
            let mut names: Vec<String> = specs.iter().map(|s| py_name(s.id.split_once('.').map_or(s.id, |(_, m)| m))).collect();
            names.sort();
            let n = names.len();
            names.dedup();
            assert_eq!(names.len(), n, "{area}");
        }
    }

    #[test]
    fn the_checked_in_client_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("clients/python/leadstinger.py");
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            on_disk == generate(),
            "clients/python/leadstinger.py is stale: run `cargo run --bin leadstinger-cli -- commands --python > clients/python/leadstinger.py`"
        );
    }
}
