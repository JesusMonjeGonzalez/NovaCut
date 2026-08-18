# Public Release Status

Status: **engineering alpha**.

This branch is a portfolio publication candidate, not a `1.0` release. The
native harness and build are the required local gates. Hosted CI verifies the
application build and synthetic media corpus. The full native harness includes
CoreImage compositor checks that require the local macOS runtime. The corpus
must be redacted or synthetic, and its runner fails when no synchronization
measurement was made.

Still open before a distributable release:

- real heterogeneous media corpus with documented provenance;
- external-editor opening checks for EDL/FCPXML;
- UI recovery, relinking and cancellation checks;
- signed/notarized packaging;
- a third-party dependency and asset inventory for packaged distribution.
