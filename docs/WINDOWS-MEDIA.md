# Windows Media Browser

The left **Medios** panel derives its contents from the current timeline clips.
There is no independent asset library, import database, or new project format.

## Eight Capabilities

1. **Type filters:** Todos, Video, Audio, and Generados. Video includes sources
   with both picture and sound; Audio is audio-only. Titles, adjustment layers,
   nested sequences, and pathless clips are generated items and remain separate.
2. **Offline-only filter:** Offline shows sources whose original path is not a
   file, even when a proxy is available. Generated items are never offline.
3. **Proxy-state filter:** Show all sources, sources with an unlinked use, an
   available linked proxy, or a missing linked proxy. A mixed source can match
   multiple states; matching one use retains the entire source row. Generated
   items have no proxy state and only appear under Proxy: todos. Availability
   does not certify proxy freshness, decode support, or suitability for export.
4. **Deterministic sorting:** Name (case-insensitive), duration, or usage count,
   ascending or descending. Ties use source path and then the first timeline-use
   index, ascending regardless of direction. Generated items participate in the
   same ordering. Duration is the maximum known source duration across uses,
   falling back to each use's source out-point; generated duration is its timeline
   duration. This does not probe files for missing metadata.
5. **Token search:** Case-insensitive, whitespace-separated tokens must all occur
   in the full source path or the collected display names of its uses. Token order
   does not matter. Tokens may match different names on the same source. There is
   no glob, regex, or quoted-phrase syntax. Search combines with all smart filters.
6. **Complete usage accounting:** Sources are grouped before filtering. Row counts,
   duration, proxy counts, and actions include every top-level timeline use, even
   when only one use's name matches. The summary reports visible sources/items and
   their complete use counts. Disabled clips count too; nested contents are not
   recursively enumerated. Grouping uses the stored PathBuf, without filesystem
   canonicalization, symlink resolution, or Windows case-alias normalization.
7. **Select all uses:** Right-click a row and choose Seleccionar los N usos. This
   replaces the timeline selection with all uses of that source, preserving a
   primary use from that source when possible. It works across tracks and does not
   edit clips. Any selected use highlights its source row.
8. **Previous/next use navigation:** The row context menu visits uses in timeline
   start, track, then clip-index order, wrapping at either end. If the primary
   selection is from another source, next starts at the first use and previous at
   the last. These commands select one use, seek, follow the playhead horizontally,
   and refresh the preview. They are disabled for single-use rows. Ordinary click
   visits the current use of a source, or its first use; double-click still opens
   the source monitor.

## Supporting UI

The compact, wrapping toolbar retains the existing dark theme. Hover a row for
its full name/path, type, duration, complete usage count, original availability,
per-use proxy counts, and current use's track/timecode. Names elide to fit the
available width, reserving space for OFFLINE warnings. Counts stay on the second
line so an offline warning does not replace them.

The search clear button only clears text. Restablecer resets search, smart filters,
and sorting to all sources/name ascending. A filtered empty state offers the same
reset; an empty project instead prompts for media. Browser controls are session
UI state, not persisted project data.

Original/proxy file checks share a two-second cache, checking each distinct path
at most once per cache interval when the panel renders. New paths are checked
immediately. External changes to an already cached path appear on the next render
after expiry. No directory scanning or media decoding is added. The cache does
not change existing inspector/proxy diagnostics elsewhere in the application.

## Verification

Pure helper tests live in `src/windows/media_browser.rs` and cover token search,
complete grouping, generated/audio filters, mixed proxy states, sorting, and
timeline navigation. They can run on macOS using `rustc --edition=2021 --test`
without the Windows UI dependencies. The Windows binary and its tests also need
the project cross-check:

```sh
rustup run stable cargo check --locked --offline --target x86_64-pc-windows-gnu --features windows-host --bin novacut-windows --tests
```

On Windows, manually check a source used on several tracks, a missing original,
mixed per-use proxy links, generated items, long paths, and a narrow panel. Verify
that filtering never changes source counts, source-wide selection reaches every
use, and navigation wraps while keeping the playhead visible. Cross-checking alone
does not execute Windows UI tests or verify rendering, playback, or drag/drop.
