# Security Policy

## Scope

NovaCut is an engineering-alpha macOS application. Projects, media paths,
transcripts and optional assisted-editing prompts may contain sensitive material.

The repository does not accept recordings, `.editorcito` projects, transcripts,
provider credentials or local model configuration as public fixtures.

## Boundary

- Local assisted editing keeps requests on the Mac when local mode is selected.
- Optional remote providers receive only the metadata described in `README.md`.
- Treat filenames, transcripts and markers as sensitive before enabling a remote provider.
- The application is not a secure multi-user service and has no production release yet.

## Reporting

Do not open a public issue containing a credential, private media, transcript or
reproduction data. Use a private GitHub security advisory or contact the
repository owner through GitHub with a minimal redacted description.

## Release rule

Builds, screenshots and synthetic tests are not evidence that private media is
safe to publish. A release candidate must pass a secret scan, keep the corpus
redacted, and document the exact verification commands and known limits.
