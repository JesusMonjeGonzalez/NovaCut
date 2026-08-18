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
release.
