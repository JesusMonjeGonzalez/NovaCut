# NovaCut

Native macOS video editor built with Swift, SwiftUI, AVFoundation and a growing
Rust core.

NovaCut models editing operations in integer frames over a rational timebase.
The current application supports multipista editing, transcript-based cuts,
multicam workflows, compositing, color tools, audio DSP and reversible exports.

## Highlights

- Ripple, roll, slip and slide edits with linked audio/video synchronization.
- Transcript search, filler-word review and silence removal.
- Multicam synchronization, live angle switching and flattening.
- Titles, masks, blend modes, RGB curves, LUTs and adjustment layers.
- Noise gate, EQ, multiband compression, limiter, reverb and delay.
- Export queue with atomic destination replacement and cancellation safety.
- Local model integration plus an optional cloud provider with explicit context
  disclosure; video frames and audio remain local in cloud editing mode.

## Build

Requires a recent Xcode installation and macOS SDK.

```bash
./build-mac.sh
open build/Editorcito.app
```

The application bundle keeps the current working name `Editorcito.app` while
the project and repository use NovaCut.

## Verification

```bash
./probar.sh
./probar.sh /path/to/video.mp4
```

The harness covers timeline operations, transcript editing, color processing,
audio DSP, multicam synchronization, retiming, scopes and composition behavior.
Some Swift concurrency and deprecated AVFoundation warnings remain visible and
are tracked as migration work rather than suppressed.

## Architecture

```text
src/ui/    native macOS application and media pipeline
src/core/  Rust foundation for future cache, render and codec work
tests/     executable Swift verification harnesses
docs/      architecture, formats, roadmap and user guidance
```

## Status

NovaCut is an active engineering project, not a replacement for a production
NLE. See `docs/FORMATOS.md` and `docs/ROADMAP.md` for current codec and workflow
boundaries.
