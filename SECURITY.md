# Security Policy

## Supported versions

The supported compiler is the `version` in the root `Cargo.toml` on `main`. That version is 0.9.0. A security fix is a commit on `main`. Older commits stay as they were published.

| Version | Supported          |
| ------- | ------------------ |
| 0.9.0   | :white_check_mark: |
| < 0.9.0 | :x:                |

`scripts/check_docs.py` fails when the version sentence or the supported row disagrees with `Cargo.toml`.

## Reporting a vulnerability

Report a vulnerability in a private GitHub security advisory:

https://github.com/Thrainkrilleve/aven/security/advisories/new

Include:

- The commit you built (`git rev-parse HEAD`).
- The command you ran and the input file, or the URL on `avenc.dev`.
- The behavior you observed.

The advisory is updated when the report is accepted or declined. An accepted report reproduces on `main`, and the fix is a commit on `main`. The advisory names that commit when the fix is on `main`. A report that does not reproduce on `main` is declined on the advisory with that result.
