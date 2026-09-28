# Third-Party Notices

NovaCut does not vendor third-party source code or media fixtures in this
repository.

The native application uses Apple system frameworks, including AVFoundation,
AVAudio, Core Image, Core Media, Core Video, Speech, Vision and SwiftUI.
Those frameworks are provided under Apple's platform terms.

The experimental Rust core declares these crates in `Cargo.toml`: `serde`,
`serde_json` and `parking_lot`. Their licenses and transitive dependencies are
resolved by Cargo and must be included in any packaged distribution inventory.

This file is not a substitute for the generated dependency report of a binary
release. The Windows package carries
`docs/licenses/THIRD_PARTY_LICENSES-Windows.html`, generated with
cargo-about 0.9.2 over `Cargo.lock` for the `windows-host` feature by
`bash tools/license-report.sh`; `bash tools/license-report.sh --check` verifies
that the committed report matches the lockfile. The macOS application links no
Rust crates and uses Apple system frameworks under Apple's platform terms.

The Windows installer and application download two optional components at the
user's request, each pinned to a version and SHA-256 digest: FFmpeg 9.0.2
("essentials" build by gyan.dev, GPLv3; its license is installed next to the
binaries as `FFmpeg-LICENSE.txt`) and whisper.cpp 1.9.2 with a `ggml` Whisper
model (MIT, from github.com/ggml-org/whisper.cpp and
huggingface.co/ggerganov/whisper.cpp). If the Microsoft Visual C++ runtime is
missing, it is installed from Microsoft under Microsoft's terms after checking
its Authenticode signature. None of these are redistributed in this
repository or in the NovaCut packages.
