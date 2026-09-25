# Windows Batch Editing

## Delivered

- Selecting more than one timeline clip replaces the single-clip inspector with a batch inspector. A single selection retains the existing inspector. Each field displays its common value, `Mixto`, or `No compatible`, considering compatible, unlocked targets only.
- Transform editing sets checked X/Y position, scale, rotation, and opacity fields to absolute values across compatible video clips. Unchecked fields remain untouched.
- Color editing sets checked exposure, contrast, saturation, vignette, and blur fields. Adjustment layers support these color operations, but not batch transform/audio/fades.
- Audio editing sets checked gain, stereo pan, and mute fields on clips with audio. Values use the existing inspector's ranges.
- `Aplicar campos marcados` commits the staged fields together. Editing controls alone does not modify the project. Staging clears when the selection or document generation changes.
- Copy attributes captures the primary clip. Paste opens a category dialog targeting the entire selection: transform, color/effects, audio (including mute), fades, and label. No category is selected initially. The dialog remembers category choices, snapshots its source, and cancels if the target selection or document generation changes.
- Color/effects paste and reset include wheels, curves, chroma, LUT reference, mask, and blend mode as well as the five scalar color controls. Transform does not include animation.
- Batch fades support separate entry/exit values. Quick fade now targets the selection with one second at each edge. Every written fade is clamped independently to half that clip's current timeline duration, including retiming.
- Reset restores only checked categories to clip defaults. It does not remove keyframes, retiming, transitions, or media references.
- Batch results report changed, unchanged, locked, incompatible, and partially compatible clip counts. Partial means some requested fields/categories were unsupported; it can overlap with changed or unchanged counts. Titles and nested sequences are skipped, not recursively edited. Audio-only clips reject visual categories; silent clips reject audio edits.
- Context copy, copy attributes, duplicate, paste attributes, delete leaving a gap, quick fade, and enable/disable preserve the selection when invoked on one of its members. Copy attributes uses the clicked member as source. Invoking these on an unselected clip targets that clip instead. Other context commands retain single-target behavior, including ripple delete.
- Each effective batch Apply, paste, reset, or quick fade calls the existing edit transaction once. No-op operations do not create undo entries, clear redo, dirty the document, or request preview work. An already pending single-clip inspector edit remains a separate undo step through `finish_edit`.

## Preservation And Limits

Batch attribute operations preserve source/proxy paths, source metadata, timeline placement, track, trim points, speed, speed ramps, freeze-frame source time, keyframes, transitions, and nested contents. Color paste intentionally replaces the LUT reference when that category is selected. Existing keyframes may override static transform values at playback; batch editing does not rewrite animation curves.

The UI uses staged absolute values, not relative offsets or live multi-clip dragging. Reset is exposed in the multiselect inspector. Category paste replaces a whole chosen category, not individual effects within that category. Locked clips are never changed by batch attribute operations. Existing structural commands such as duplicate/delete retain their own transaction and compatibility behavior; they do not use the new attribute report. Enable/disable and primary-attribute copy remain accessible from the batch inspector.

The paste dialog is a nonmodal window. Selection/document changes cancel it rather than silently retargeting it. Existing context menus on locked clips still expose only copy operations.

## Verification

`src/windows/batch.rs` contains ten inline pure-helper tests covering mixed values, duplicate/stale indices, no-ops and invalid input, retimed fade limits, locks and partial compatibility, selective paste/reset preservation, mute transfer, generated clips, effects-only changes, and incompatible sources.

The Windows binary and its tests are type-checked with:

```sh
rustup run stable cargo check --locked --offline --target x86_64-pc-windows-gnu --features windows-host --bin novacut-windows --tests
```

This compiles but does not execute tests. Native macOS inclusion is not provided: the helpers share the Windows host's private clip/project model, and that host imports Windows-only APIs and target-scoped UI dependencies. No Cargo changes or duplicate test-only model were introduced. Interactive Windows UI, playback, and undo/redo validation remain to be performed on Windows.
