# ADR 0007: Apache-2.0, and no code from Artisan

## Status
Accepted (2026-11-08)

## Context
RoastMCP should be open and attractive for contributors, including roaster
manufacturers who might add drivers. Artisan, the reference in this field, is
licensed AGPLv3 since 5.0; copying or porting its code would make RoastMCP a
derivative work under AGPL.

## Decision
- RoastMCP is licensed under the **Apache License 2.0**.
- **Clean room for Artisan:** no Artisan source code is copied, translated or
  closely paraphrased. Reading Artisan's source, settings files and logs to
  learn protocol and file-format facts is fine; those facts are documented in
  our own words in `docs/research/` with sources.
- Third-party models or data (for example a first-crack classifier) are only
  used if their licence is compatible and recorded.

## Consequences
- Permissive licence with a patent grant; closed forks are allowed.
- Algorithms (rate of rise, event detection, controllers) are our own
  implementations.
- Reviewers check pull requests for code taken from Artisan.
