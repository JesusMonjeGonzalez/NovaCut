# Cargo dependency audit

## Scope and evidence

- Audit date: 2026-09-25; local working tree, including pre-existing changes.
- Tool: `cargo-audit 0.22.2`, installed with `cargo install cargo-audit --version 0.22.2 --locked --root /var/folders/tr/ysy_sv311c72j5yt6fc493yc0000gn/T/opencode/novacut-cargo-audit` after verifying the temporary parent directory. No global binary installation or PATH change.
- RustSec database: https://github.com/RustSec/advisory-db, freshly fetched by the audit, commit `913a741345c1df04dd8ee83f4304f439caa30ccc`, dated `2026-09-25T13:56:01+02:00`; 1,269 advisories.
- Lockfile: 376 dependencies; SHA-256 `499431b6f3e25a7e08b9e6bee9e2c253a7c8fef6b11d23675ad9fa7ccd092b01`.
- Full lockfile scan, without OS/architecture filters or ignored advisories. Yanked-package checking was not disabled; no yanked warning was reported.
- Runtime: Cargo 1.98.0, rustc 1.98.0 (Homebrew), `aarch64-apple-darwin`.

Commands used from the repository root:

```sh
AUDIT=/var/folders/tr/ysy_sv311c72j5yt6fc493yc0000gn/T/opencode/novacut-cargo-audit/bin/cargo-audit
DB=/var/folders/tr/ysy_sv311c72j5yt6fc493yc0000gn/T/opencode/novacut-rustsec-db
"$AUDIT" audit --db "$DB" --json
"$AUDIT" audit --db "$DB" --no-fetch --deny warnings
```

## Findings

The initial JSON audit exited successfully: **0 known vulnerabilities, 1 unmaintained warning**. This is not a clean maintenance result or a guarantee of safety. The subsequent strict audit against the same database failed with `1 denied warning found!`.

| Advisory | Locked package | Classification | Resolution |
| --- | --- | --- | --- |
| [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192) | `ttf-parser 0.25.1` | Unmaintained; published 2026-06-28; no CVSS score or CVE alias in the advisory | Open; RustSec lists no patched or unaffected versions |

The advisory states that the author will no longer maintain the crate or provide fixes, referencing https://github.com/harfbuzz/ttf-parser/issues/217. It recommends the maintained `skrifa` font parser. It does not identify a specific exploitable vulnerability.

Applicability was confirmed with `cargo tree --locked --offline --features windows-host --invert ttf-parser` on macOS:

```text
ttf-parser 0.25.1
  owned_ttf_parser 0.25.1
    ab_glyph 0.2.32
      editorcito-core (optional direct dependency)
      epaint 0.31.1
        egui 0.31.1
          eframe / egui-winit / egui_glow 0.31.1
```

This is a runtime dependency of the `windows-host` feature, which is also buildable on macOS. `src/windows/subtitulos_animados.rs:87-96` reads font files and parses them through `ab_glyph::FontArc`; the UI also uses egui fonts. The warning is therefore relevant to the enabled host, not merely an unused lockfile entry. It is absent from the default-feature core graph: the inverse-tree query without `windows-host` returned no matching package.

No dependency changes were made: there are no reported vulnerabilities to patch, and no advisory-listed patched version for the maintenance warning. Replacing `ttf-parser` with `skrifa` is not a drop-in manifest substitution. A follow-up must evaluate maintained font-stack versions or migration, address both the direct `ab_glyph` usage and egui's transitive path, and verify font layout/rendering. Source/API migration is outside this audit's authorized edits. No `src` files were edited and no advisory was suppressed.

## Verification and limits

| Check | Observed result |
| --- | --- |
| `cargo test --locked --offline --lib` | 53 passed, 0 failed, 0 ignored |
| `cargo test --locked --offline --features windows-host --bin novacut-windows` | 132 passed, 0 failed, 1 ignored |
| Strict RustSec audit with `--deny warnings` | Failed on RUSTSEC-2026-0192, intentionally not suppressed |
| `cargo tree --locked --offline --all-features --target all --invert ttf-parser` | Blocked: `alsa 0.9.1` was not cached and offline mode prevented downloading it |

The ignored host test was `tests::lut_paths_render_with_real_ffmpeg` (requires real FFmpeg with lavfi and lut3d). Passing tests were run on macOS, not native Windows or Linux. The all-target tree limitation does not restrict the full lockfile RustSec scan. No native cross-platform runtime or packaged-artifact validation was performed.

This audit covers RustSec matches in Cargo.lock, not external FFmpeg/Whisper binaries, Swift/system frameworks, application logic, or a complete supply-chain/security review. It supersedes only the earlier statement in `docs/AUDIT-2026-09-25.md` that cargo-audit was unavailable; that existing report was not edited.

## Proposed CI gate

Proposal only; no workflows were changed. Add a dedicated job on pull requests and a weekly schedule, with read-only repository permissions and checkout pinned to a reviewed commit. Pin the audit tool, but fetch current RustSec data on every run:

```sh
cargo install cargo-audit --version 0.22.2 --locked --root "$RUNNER_TEMP/cargo-audit"
"$RUNNER_TEMP/cargo-audit/bin/cargo-audit" audit \
  --file Cargo.lock --db "$RUNNER_TEMP/rustsec-db" --deny warnings
```

Do not add `--ignore`, `--stale`, `--no-yanked`, or error-tolerant execution to make the job green. This proposed gate currently fails on the open maintenance advisory. Resolve the font-stack maintenance issue before expecting a passing strict gate. Retain the audit output/database revision as CI evidence and review tool-version updates explicitly. A pinned tool does not mean a frozen advisory database.
