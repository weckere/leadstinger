# A project board for agents

LeadStinger was built by several Claude Code sessions at once, with one person steering.
They coordinated on a **project board**: one web page holding the milestones, tasks,
decisions, history, agents, blockers and metrics. It is a claude.ai artifact backed by a
small shared database. People watch the page update live, and agents read and write the
same rows with Claude Code's `ArtifactData` tool. [PROJECT.md](../PROJECT.md) is a
snapshot of LeadStinger's board taken for the release.

This folder is that board as a template you can reuse:

| File | What |
|---|---|
| `template.html` | the page, with `{{PLACEHOLDERS}}` for your project's name and links |
| `example.json` | the values LeadStinger used |
| `render.py` | fills the template: `python3 render.py my-project.json board.html` |

## Why a database and not a file in the repo

- Sessions work on different branches and worktrees. A status file in git would only be
  current on one branch at a time, and it would conflict on every merge.
- The page updates live while you watch it, and nobody has to republish anything.
- Every document has a version, so two agents writing the same row can't silently
  overwrite each other. A write pinned with `if_version` fails if the row changed since it
  was read.

## Setting one up

1. Copy `example.json`, change the values, and run `render.py` to make `board.html`.
2. Ask Claude Code to publish `board.html` as an artifact with the `db` capability. The
   page reads its data with `claude.use("db")` and shows a sign-in banner when there's
   none.
3. Put the protocol below in your repo's `CLAUDE.md` (or `AGENTS.md`), with the board's
   URL, so every session reads and writes it.
4. Seed it: ask a session to create the first milestones, tasks and decisions with
   `ArtifactData`. Data changes never need a republish; only changes to the page do.

The data loads only for viewers signed in to claude.ai, even through a share link. For
readers without an account, export the collections on a schedule (`ArtifactData` reads
with `out_dir`) and render them somewhere public.

## The protocol

1. **At session start, read the board.** List `milestones`, `tasks`, `agents` and
   `blockers`. This replaces working out the status from git logs and messages.
2. **Register** in `agents`, with the session's short name as the doc id: role, current
   task, branch, worktree, `status: "active"` and `last_seen`. A row that stays `active`
   for hours without an update shows as stale.
3. **Claim a task before working on it.** Set `owner`, then `status: "claimed"`, then
   `in_progress`. **Skip tasks someone else has claimed;** this is the only lock there is.
   If the work has no task row, create one and claim it.
4. **Append an `events` row on every meaningful step and every commit.** The history view
   is built from it, and a later session reads it to learn what happened. Never edit or
   delete an event.
5. **Record every decision with its reasoning** in `decisions`, including the alternatives
   that were turned down. A later agent that disagrees with a choice should find out why
   it was made before reversing it.
6. **When you finish or get blocked, update your `agents` row and the task status.** If
   blocked, raise a `blockers` row saying what you need and from whom. Keep `milestones`
   progress and `metrics` current.
7. **Never delete other agents' rows.** Change someone else's row only to resolve what they
   asked for, and log an event saying so.

## How to write

- Read before you write. Every read returns a `version`; pass it as `if_version` on `set`,
  `update` or `delete` of an existing document, and leave it out only when creating one.
  On a conflict, re-read and redo the change.
- Use `update` (merge) rather than `set` (replace) for existing rows, so you don't drop
  fields you didn't mean to touch. Use `batch` for more than a couple of writes.
- Timestamps are UTC ISO 8601 with a `Z`, e.g. `2026-10-18T03:44:34Z`. The page sorts on
  them.
- Use plain text in every field. The page shows it as text, not HTML or Markdown.

Example: claim a task.

```json
{"action": "update", "url": "<your board's URL>",
 "collection": "tasks", "doc_id": "t-m6-controller", "if_version": 1,
 "data": {"owner": "api-work", "status": "in_progress", "branch": "api-work", "updated": "2026-10-18T05:00:00Z"}}
```

## Collections

| Collection | Doc id | Fields |
|---|---|---|
| `milestones` | `M0`, `M1`, … | id, order (number), title, status, owner, progress (0–100), depends_on (array of ids), summary, branch, updated |
| `tasks` | `t-m<N>-<slug>` | id, milestone, title, status, owner, branch, notes, updated |
| `decisions` | `D-NNN` | id, date (YYYY-MM-DD), decision, reasoning, alternatives, decided_by, milestone |
| `events` | `<YYYYMMDDTHHMMSSZ>-<agent>` | ts, agent, kind, text, commit, milestone |
| `agents` | session short name | name, session_id, role, current_task, branch, worktree, status, last_seen |
| `blockers` | `B-NNN` | id, title, detail, status, milestone, needs, raised_by, raised, resolved, resolution |
| `metrics` | `tests-<module>` or `plat-<os>-<arch>` | kind `"tests"`: module, tests, branch, note, updated. kind `"platform"`: order, platform, build, tests, runs, audio, where, updated |

Status values:

- milestones: `todo`, `in_progress`, `review` (finished on a branch, waiting to merge),
  `blocked`, `done`
- tasks: `todo`, `claimed`, `in_progress`, `blocked`, `review`, `done`
- agents: `active`, `idle`, `blocked`, `finished`
- blockers: `open`, `resolved`
- event kinds: `start`, `claim`, `commit`, `merge`, `decision`, `blocked`, `finish`, `note`
- platform cells: `yes`, `partial`, `untested`, `no`, `blocked`

The dependency graph on the page is drawn from `milestones.depends_on`, so keep it
accurate when the plan changes.
