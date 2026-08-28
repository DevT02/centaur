# Security policy

Centaur reads local repositories and writes reviewed patches, so path containment, complete validation, undo history, redaction, and transport authentication are security boundaries.

## Report a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/DevT02/centaur/security/advisories/new). Do not open a public issue for a suspected vulnerability and do not include real credentials, private source code, or personal paths in a report.

Include only the minimum reproduction needed:

- Centaur version or commit
- operating system
- affected command or MCP transport
- expected safety behavior
- observed behavior
- a synthetic fixture when possible

## In scope

- Workspace escape through traversal, symlinks, or undo data
- Partial writes after validation or snapshot failure
- Dry-run side effects
- Secret redaction changing source files
- Unauthenticated or non-loopback HTTP behavior
- Source drift overwriting later user changes

Feature requests, ordinary crashes, and documentation problems can use the public issue forms.

## Disclosure

Please allow time to reproduce and fix the issue before public disclosure. A fix is not complete until the relevant regression test passes on the supported CI platforms.
