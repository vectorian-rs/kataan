# Working on kataan

Conventions for anyone — human or agent — changing this repository. Short on
purpose. Every rule here is written down because breaking it cost something.

For using kataan from an agent, see [`docs/kataan-agent-guide.md`](docs/kataan-agent-guide.md).
That is a different document: it describes the tool, this describes the work.

## The gate

`mise run check` — fmt, clippy, the Rust tests, `astro check`, and the web
build. It must pass before a commit. Nothing else is a substitute for it.

Source files stay under 800 lines. This one is a convention, not enforced by the
gate, so it only holds if you check it. When a file grows past, split it by
concern rather than by byte offset — `apps/web/src/styles/global/` is the worked
example, and it was re-cut on rule boundaries precisely because the first split
was by offset and meant nothing.

## Measure before you optimise

**Find a control.** `rebuild-indexes` took 9.3s and `validate` took 0.20s while
walking the same tree, reading the same files and parsing the same TOML. That
single comparison localised the cost to "what rebuild does that validate does
not" — writing — without a profiler. Look for the operation that does almost the
same work, and diff them.

**Measure the thing you are about to change, especially when handed a target.**
A request to cache checksums on mtime sounded reasonable and would have bought
~10%: the vault is 4 MB across 1,592 files, and blake3 runs at ~1 GB/s, so
hashing was never the cost. One command established that before any code was
written. A plausible target is the easiest way to spend a day on nothing.

**Measure the right signal.** `rebuild-indexes` was reported as "does not
reserialize any more" because `git status` was clean afterwards. The writes were
happening; they just produced identical bytes. Clean diff is not no I/O. Ask
what would actually be observable if the thing were true.

**Measure at real scale.** The 9.2s write only appears at 787 documents. Use an
`rsync` copy of a real vault, not a fixture with four notes.

## Verify a fix by removing it

Before committing a fix with a test, stub the fix out and confirm the test
fails. Two tests in this repository passed with their fix disabled — a
saturation test whose latency bound was too loose to discriminate, and a
`create_document` check that passed because a predicate name was unknown rather
than because the type was refused. Both looked like evidence and were not.

Where the failure is silent rather than loud, assert on the mechanism, not the
symptom: the write-skip test compares the file's **inode**, because a rewrite
with identical bytes satisfies a content check while still paying the fsyncs the
change exists to avoid.

## Check the premise before acting on it

Issues, comments and TODOs go stale, and this repository moves fast enough that
several have. Before implementing one, verify what it claims still holds —
`route_token` was gone, a `resolve` tool no longer existed, a CSS duplicate list
had been fixed by an unrelated split, and one issue's stated purpose ("settle
this before #6/#7/#8 land") had expired because all three had landed.

When a claim turns out to be stale, say so on the issue. That is part of the
work, not an aside.

## A rule written twice will drift

Both bugs found in the 2.0.0 review came from the same shape: a rule expressed
in two places, where one was updated and the other was not.

- `declares_a_reference` decided whether to load the document index by looking
  at top-level fields only. When field validation learned to recurse into
  tables, nested references became unsatisfiable — the index was never loaded,
  so every target was reported as not existing.
- A depth guard was pasted into three walkers, and the fourth — the one
  `validate` itself uses — was missed.

Extract the rule, or accept that the copies will diverge. There is no third
outcome.

## The vault is live

`~/code/home/knowledgebase/snuffbox` is real, in use, and edited while you work.

- Never run a mutating command against it. `rsync -a --exclude node_modules` a
  copy and work there. Read-only `validate` and `documents` against the original
  are fine and are the best final check.
- A long-lived `kataan-server` runs on **port 3001**. Never
  `pkill -f "kataan-server"` — it matches. Start probe servers with an explicit
  `--bind` and kill by that: `pkill -f "bind 127.0.0.1:3019"`. It is safe
  precisely because the long-lived server takes the default and carries no
  `--bind` in its command line. Run `pgrep -fl kataan-server` first and confirm
  what your pattern would match.
- Expect it to change under you. Files move mid-session; a 404 is as likely to
  be the vault as the code.

## The frontend has no test harness

`astro check` type-checks and nothing else. Behaviour is verified by driving a
real browser against a real vault. Assert on observable state — the URL, the
breadcrumb, computed styles, `history.length` — and re-check after a reload,
because "it works until you refresh" is the common frontend failure here.

When a refactor must not change behaviour, prove it rather than eyeballing it.
The stylesheet split asserted that the built bundle was byte-identical: same
size, same md5, same content hash.

## Traps this codebase has already sprung

- **A module-level `const` in `dashboard.ts` is `undefined` during boot.** The
  bundler lowers `const` to `var`, so there is no temporal-dead-zone error —
  the comparison silently runs against `undefined`. Every deep link fell through
  to the default folder with nothing in the console. Declare anything boot reads
  above the boot block.
- **File creation modes are masked by the umask; `chmod` is not.** Asking
  `tempfile` for 0664 under a 022 umask silently yields 0644. Set the mode after
  creation.
- **`rename` replaces the inode**, so the destination's mode and owner are lost
  unless carried over. Ownership cannot be restored without privileges.
- **`Path::join("")` appends a separator**, producing `projects//x.toml`.
- **`cp -p`, `rsync -a` and `tar -x` preserve mtimes**, which is why mtime is not
  a safe cache key for anything this repository checksums.
- **A folder index is a document of its type.** `people/index.toml` is a
  `person`, so per-type counts include it. Report the distinction rather than
  subtracting it — kataan cannot tell a container from a real entity that owns
  edges.

## Commit messages

Say what was wrong and why the fix is shaped the way it is, not what changed —
the diff already says that. Include the measurement if there was one, the
alternative you rejected and why, and anything you deliberately left undone.
A commit that records "this test passed with the fix removed, so I retightened
it" is worth more later than one that records "fixed the test".
