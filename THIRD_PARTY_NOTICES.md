# Third-Party Notices

NovaCut includes the `beguiling-drafter` RNNoise model from
[GregorR/rnnoise-models](https://github.com/GregorR/rnnoise-models), embedded
in the Windows executable for local voice cleanup (`assets/modelos/voz.rnnn`).
The upstream README states that, except for its tools directory and README,
the work is not creative and is not subject to copyright.
Model SHA-256: `ae3f7411e1e6a884f839a4a145c394408398f09854dbc1216ee02faafc98a17b`.

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
