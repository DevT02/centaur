# User workflow research and implementation backlog

This document freezes the discovery work completed across 23 user cohorts. It is the
product evidence baseline for deciding what Centaur should build next. Discovery can
resume when usage data or a concrete request contradicts this baseline; until then,
implementation should work down the ranked backlog.

Centaur's product boundary remains deliberately small:

- It selects and exports local repository evidence for a model.
- It validates every proposed file edit before writing any edit.
- It records workspace-scoped recovery state before writes begin.
- It does not silently acquire shell, cloud, registry, device, database, or browser
  authority.

## What users consistently want

Across otherwise different workflows, users repeatedly asked for the same outcomes:

1. Give the model the right evidence, not merely more files.
2. Explain what will change before any write or external effect.
3. Refuse partial, stale, ambiguous, or unverifiable operations.
4. Preserve exact bytes and user-owned state.
5. Distinguish source edits from databases, hardware, cloud state, registries,
   installed applications, and other effects that file undo cannot restore.
6. Produce a receipt that another person can inspect or reproduce.
7. Work offline or locally by default, with cloud actions remaining explicit.
8. Fit existing tools: terminals, editors, MCP clients, CI, package managers, and
   platform-native review flows.
9. Degrade accessibly when color, pointer precision, clipboard access, interactivity,
   network access, or a large context window is unavailable.
10. Keep generated files, public contracts, configuration layers, and release
    artifacts tied to their real sources of truth.

## Existing foundation to preserve

These are Centaur's strongest current invariants and should not be weakened while
adding features:

- Every patch is validated before any patch is written.
- Paths are contained within the active workspace, including through symlinks.
- A failed undo snapshot prevents writes.
- Dry runs are side-effect free.
- Redaction changes only a temporary export copy.
- CRLF and UTF-8 BOM behavior is regression tested.
- Undo history is scoped to the workspace that created it.
- Browser workflows remain user-driven; Centaur does not upload repositories itself.

## Shared architecture discovered

The cohorts exposed a small set of reusable models. Implement these once, then let
domain workflows consume them.

### Semantic ownership graph

Classify files and state as source, generated output, vendored input, fixture,
configuration, secret material, build artifact, release artifact, or external state.
Record the source of truth and permitted edit direction for each class.

### Evidence manifest

Every export should say what was included, omitted, summarized, truncated, redacted,
or unreadable, with paths, reasons, byte sizes, checksums, and session lineage.
Absence must be visible evidence rather than silence.

### Change-set topology

Changed and staged exports need modifications, additions, renames, and deletion
tombstones. A deleted file can carry essential ownership, permission, API, migration,
or security context even though it no longer exists in the worktree.

### Effect-aware execution plan

Separate:

- workspace file writes;
- generated-file regeneration;
- database migrations and data changes;
- device and firmware writes;
- cloud and infrastructure changes;
- registry, package, and release mutations;
- installed application and operating-system integration;
- browser and native-host registration.

For each effect, state validation, approval, recovery, and whether Centaur undo applies.

### Artifact sensitivity taxonomy

Classify credentials, private keys, signing containers, public certificates, public
identifiers, personal data, generated artifacts, and release artifacts. Redaction and
export policy must follow the class instead of treating every opaque file alike.

### Effective configuration graph

Show layered inputs, precedence, generated configuration, environment overrides,
policy, and the effective value. Editing a visible generated result without tracing
its source is usually the wrong change.

### Generated-source ownership

Record generator, inputs, version, command, and output checksum. Public generated
bindings, declarations, schemas, docs, and firmware tables are contracts, not token
budget noise.

### Public contract graph

Track exported APIs, CLI flags, configuration schemas, protocols, file formats,
features, documented behavior, error contracts, and compatibility baselines.

### Release unit graph

Map independently shipped packages, applications, extensions, generated SDKs,
registries, internal dependencies, version policy, and publication order.

### Installed-state graph

Separate immutable program files, user data, caches, credentials, logs, services,
PATH entries, shortcuts, file associations, update state, and shared dependencies.
Binary rollback and user-data rollback are different operations.

### Permission-to-capability graph

Tie every declared extension, platform, cloud, or device permission to exact code,
a user-visible feature, consent behavior, and disclosure. Flag unused or widened
authority.

### Trust chains

Receipts should connect:

- source commit to build to artifact to release to installed binary;
- page to content script to extension worker to native host to workspace;
- migration source to generated plan to applied external state;
- prompt and model context to proposed patch to reviewed write.

## Ranked implementation backlog

Priority zero protects data or authority and should precede convenience work.

### P0: integrity and trust boundaries

1. Replace the current-directory self-updater with an explicit Centaur source.
2. Define a conflict-safe Patch Protocol v2 with escaping, versioning, and complete
   malformed-block rejection.
3. Add compare-and-swap write validation so files changed after review cannot be
   overwritten.
4. Add a crash journal for multi-file application and recovery.
5. Make export and patch handling byte faithful for encodings, BOMs, newlines,
   executable bits, and non-text artifacts.
6. Make secret scanning truthful about unreadable and binary files; block known
   credential and signing containers instead of silently skipping them.
7. Add content-safe matching and explicit ambiguity reporting.
8. Make undo drift-aware so it never overwrites later user work silently.
9. Render a real diff and require approval over exact changes.
10. Classify external effects and state clearly that file snapshots cannot undo them.
11. Keep any local-LLM repair path transactional and subordinate to deterministic
    validation.
12. If a browser companion is built, use typed native messages, exact extension IDs,
    explicit user gestures, and a fixed workspace. Never bridge page input to a shell.

### P1: evidence quality and daily usability

1. Emit a provenance-rich evidence manifest and context receipt.
2. Include deletion tombstones, rename lineage, and reasons for every omission.
3. Discover nested manifests and ownership boundaries in changed and staged modes.
4. Classify lockfiles by application, library, fixture, and release role.
5. Add plan-then-implement and review-only recipes.
6. Add patch repair that explains stale or non-unique SEARCH text without guessing.
7. Add JSON output, stdin input, non-interactive approval, and stable exit contracts.
8. Provide accessible prompts, keyboard-only review, reduced-motion output, and
   screen-reader-safe status text.
9. Support paging and resumable session lineage without losing evidence.
10. Add coherent-snapshot exports and concurrent patch lanes.
11. Add a patch inbox with origin, target workspace, base revision, and expiry.
12. Verify documentation snippets, examples, links, localization, and rendered output.
13. Build artifact-first release checks for crates, packages, installers, extensions,
    mobile bundles, firmware images, and infrastructure plans.
14. Add public-contract and compatibility diffs.
15. Add installed-state and external-effect plans for application updates.
16. Add permission, data-flow, and disclosure diffs for extensions and integrations.

### P2: specialized workflows

1. Team policy packs and shared recipe definitions.
2. Opt-in downstream compatibility canaries.
3. Education mode with explanations, rubrics, hints, and evidence receipts.
4. Hardware calibration and device-fleet deployment assistance.
5. Game asset and engine-aware context packs.
6. Database migration rehearsal against disposable clones.
7. Infrastructure drift and plan evidence packs.
8. Mobile store, entitlement, signing, and device-matrix packs.
9. Library deprecation campaigns and ecosystem impact exploration.
10. Desktop release rings and incident brakes.
11. Enterprise extension policy and managed-storage profiles.

## Cohort findings

### 1. Repetitive repository work

Users want reusable recipes for recurring reviews, refactors, dependency updates, and
small fixes. They need a context receipt, task plan, exact acceptance criteria, and
patch-repair guidance when model output is stale.

Prompts:

- Build a named recipe that selects evidence deterministically and states exclusions.
- Produce a task plan with explicit verification before requesting edits.
- Diagnose a failed patch and ask for a replacement block without changing intent.

### 2. Local and offline model users

Users want Ollama and similar local models for private or disconnected work, but they
need install diagnostics, model capability checks, transactional output handling, and
clear destinations.

Prompts:

- Diagnose the local model, context limit, and executable before starting.
- Treat local model output as untrusted patch text subject to identical validation.
- Explain where exports, prompts, temporary copies, and repaired responses are stored.

### 3. Review and recovery

Users want to inspect real diffs, accept selected changes, survive source drift, and
separate planning from implementation.

Prompts:

- Render exact before/after hunks with file mode and line-ending effects.
- Refuse stale approval if the reviewed source checksum changed.
- Revert only when current content still matches the session's expected result.

### 4. Secrets and confidential repositories

Users want local-first scanning, redacted temporary copies, provenance, cleanup, and
placeholders that remain useful to a model without exposing values.

Prompts:

- Classify detected material and distinguish private secrets from public identifiers.
- Record every redaction and prove the source workspace was untouched.
- Expire temporary exports and report cleanup failures.

### 5. Automation and editor integration

Users want stdin, JSON, stable outcomes, non-interactive operation, accessible prompts,
and a guided first-run tour.

Prompts:

- Return one stable operation result across CLI, TUI, MCP, and editor entry points.
- Support explicit yes/no automation flags without silently approving writes.
- Test output with redirected streams, no clipboard, and non-interactive terminals.

### 6. Long sessions and teams

Users want paging, resumable lineage, multi-root workspaces, confidential receipts,
and team policy without a server requirement.

Prompts:

- Bind every part to session, batch, workspace, base revision, and checksum.
- Resume only when earlier parts and policy versions match.
- Keep policy files local and make conflicts visible.

### 7. Mixed code, data, notebooks, and assets

Users want byte-faithful handling, useful notebook context, asset metadata, offline
operation, and explicit trusted processors.

Prompts:

- Export notebook cells and outputs with execution order and truncation evidence.
- Describe binary assets with metadata and hashes rather than decoding them silently.
- Require an explicit processor allowlist before invoking converters.

### 8. Maintenance, upgrades, incidents, and releases

Users want change intelligence, failure capsules, dependency-upgrade evidence,
incident-safe context, and release readiness.

Prompts:

- Produce a minimal failure capsule with environment, inputs, logs, and reproduction.
- Separate a dependency's declared change from observed project impact.
- Build release evidence from the artifact that will actually ship.

### 9. Advanced version-control workflows

Users want three-way patch evidence, history archaeology, bisect assistance, backport
planning, and rebase-safe intent.

Prompts:

- Compare base, reviewed source, current source, and proposed result.
- Preserve intent and provenance when transplanting a change between branches.
- Never infer a clean merge from line similarity alone.

### 10. Open-source maintainers

Users want reproducible bug reports, issue triage, review-feedback conversion, public
privacy checks, and contributor-friendly patches.

Prompts:

- Turn an issue into reproduction, evidence request, and bounded acceptance criteria.
- Separate maintainer feedback into required, optional, and disputed changes.
- Scrub private paths and credentials from public diagnostic bundles.

### 11. Split machines and remote development

Users work across WSL, SSH, containers, Codespaces, host editors, remote shells, and
MCP clients. They need topology-aware paths and a safe courier between trust zones.

Prompts:

- Draw where source, Centaur, browser, clipboard, and model actually run.
- Translate paths only through verified workspace mappings.
- Refuse to claim that a local snapshot restores remote or container state.

### 12. Concurrency and multiple agents

Users want coherent exports, compare-and-swap writes, crash recovery, independent
lanes, and an attributable patch inbox.

Prompts:

- Capture one coherent workspace revision before packing any part.
- Revalidate every target immediately before committing the transaction.
- Queue patches with origin, base revision, target, expiry, and review status.

### 13. Documentation and localization

Users want docs treated as executable contracts, locale sets kept aligned, and visual
rendering verified rather than inferred from source.

Prompts:

- Run or compile every documented command and snippet.
- Compare locale key sets, placeholders, plural rules, and fallback behavior.
- Verify links and rendered artifacts before accepting prose-only confidence.

### 14. Education and onboarding

Learners want capability-scoped explanations, hints before answers, rubrics, and
receipts that reveal why a patch is safe.

Prompts:

- Explain only the concepts necessary for the current change.
- Offer progressive hints and a verification rubric.
- Keep the final patch format identical to production workflows.

### 15. Accessibility and assistive technology

Users need keyboard-only flows, screen-reader order, non-color status, predictable
focus, reduced motion, and output that survives high contrast and narrow terminals.

Prompts:

- Audit every action without a pointer or color distinction.
- Announce validation, approval, progress, and failure in stable textual order.
- Preserve a plain-output mode suitable for assistive tools and logs.

### 16. Embedded and firmware teams

Users need hardware context, memory maps, register ownership, generated tables,
toolchain identity, flashing boundaries, and safe fleet rollout.

Prompts:

- Connect source changes to board, target, bootloader, memory, and peripheral effects.
- Keep calibration tunable and report physical assumptions.
- Treat build, flash, fuse, and device-state changes as separate approvals.

### 17. Game developers

Users need engine-aware ownership, scenes and prefabs, generated imports, large assets,
platform builds, deterministic reproduction, and performance evidence.

Prompts:

- Trace source asset to imported artifact to scene or prefab consumer.
- Avoid editing generated engine metadata without its source asset.
- Separate code correctness from frame-time, memory, and platform certification.

### 18. Database-backed applications

Users need schema and data migrations, query plans, transaction boundaries, backward
compatibility, rollout ordering, and recovery rehearsals.

Prompts:

- Generate migration, application, and rollback effects separately.
- Rehearse against a disposable production-shaped clone.
- Prove mixed-version application compatibility during rolling deployment.

### 19. Infrastructure and platform teams

Users need effective configuration, generated plans, provider identity, drift, blast
radius, policy, secrets, and explicit apply boundaries.

Prompts:

- Compare source configuration, effective values, current state, and proposed plan.
- Treat plan files as sensitive generated artifacts.
- Never equate a source-file undo with cloud-state rollback.

### 20. Mobile developers

Users need entitlement and permission diffs, signing identity, store metadata, device
matrices, data migrations, offline behavior, and staged release evidence.

Prompts:

- Tie every mobile permission to code, feature, consent, and store disclosure.
- Test the signed bundle on supported OS and architecture combinations.
- Coordinate application rollback with on-device data compatibility.

### 21. Public library and SDK maintainers

Users need semantic compatibility review, downstream canaries, exact package contents,
generated API coherence, monorepo publication order, provenance, and bad-release
recovery.

Prompts:

- Snapshot public APIs, CLI/config contracts, features, and supported runtimes.
- Install and test the packaged artifact, not only the source tree.
- Plan yank, retract, deprecate, or corrective release actions without assuming a
  registry mutation is reversible.

### 22. Desktop applications and auto-updaters

Users need signed identity continuity, installer state, update-feed integrity,
old-to-new canaries, binary/data rollback coordination, and clean uninstall behavior.

Prompts:

- Verify product, publisher, package ID, channel, version, OS, architecture, digest,
  and signature before replacement.
- Inventory PATH, registry, services, launch agents, shortcuts, and user data.
- Test genuine older installations through update, relaunch, downgrade, and removal.

### 23. Browser-extension maintainers

Users need permission-contract diffs, browser/native trust boundaries, privacy
disclosures, ephemeral-worker reliability, storage migrations, reproducible packages,
and store rollout control.

Prompts:

- Map each permission and host pattern to code and a user-visible feature.
- Validate every page, content-script, worker, and native-host message boundary.
- Compare packaged behavior with store listing, privacy policy, and permission
  justification.

## Implementation sequence

Keep commits coherent and small enough to review:

### Phase A: stop unsafe behavior

1. Safe explicit-source updater.
2. Patch Protocol v2 and malformed-response refusal.
3. Compare-and-swap writer.
4. Crash journal.
5. Byte fidelity and truthful binary/secret handling.
6. Drift-safe undo and exact diff approval.

### Phase B: improve evidence

1. Evidence manifest with omissions and checksums.
2. Change topology with deletion tombstones and renames.
3. Nested manifest and ownership discovery.
4. Role-aware lockfiles.
5. Effective configuration and generated-source ownership.

### Phase C: make the loop composable

1. Stable operation result and JSON output.
2. Stdin and explicit non-interactive approval.
3. Resumable sessions and patch inbox.
4. Accessibility and plain-output verification.
5. Recipes for plan, review, implement, repair, and release.

### Phase D: add domain packs only from demand

Use the shared models for database, infrastructure, embedded, game, mobile, library,
desktop, extension, education, documentation, and other workflows. Do not create a
plugin system until at least two domain packs require an abstraction the ordinary
recipe format cannot express.

## Evidence sources

The research used repository evidence plus primary platform documentation. Key
references include:

- Semantic Versioning: https://semver.org/
- Cargo compatibility and publishing:
  https://doc.rust-lang.org/cargo/reference/semver.html
  and https://doc.rust-lang.org/cargo/reference/publishing.html
- npm publishing, provenance, and removal:
  https://docs.npmjs.com/policies/unpublish/
  and https://docs.npmjs.com/generating-provenance-statements/
- PyPI yanking, trusted publishing, and attestations:
  https://docs.pypi.org/project-management/yanking/
  and https://docs.pypi.org/trusted-publishers/
- Go module releases and retractions:
  https://go.dev/doc/modules/release-workflow
  and https://go.dev/ref/mod
- Maven Central immutability:
  https://central.sonatype.org/publish/requirements/immutability/
- The Update Framework threat model:
  https://theupdateframework.io/docs/security/
- Microsoft MSIX signing and deployment:
  https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview
  and
  https://learn.microsoft.com/en-us/windows/msix/desktop/managing-your-msix-deployment-targetdevices
- Apple notarization:
  https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- GitHub artifact attestations:
  https://docs.github.com/en/actions/concepts/security/artifact-attestations
- Chrome extension permissions, messaging, native messaging, storage, and updates:
  https://developer.chrome.com/docs/extensions/develop/concepts/declare-permissions
  https://developer.chrome.com/docs/extensions/develop/concepts/messaging
  https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
  https://developer.chrome.com/docs/extensions/reference/api/storage
  https://developer.chrome.com/docs/webstore/update/
- Firefox extension permissions, review, and signing:
  https://extensionworkshop.com/documentation/develop/request-the-right-permissions/
  https://extensionworkshop.com/documentation/publish/source-code-submission/
  https://extensionworkshop.com/documentation/publish/signing-and-distribution-overview/
- Microsoft Edge extension policies:
  https://learn.microsoft.com/en-us/legal/microsoft-edge/extensions/developer-policies
- Safari extension permissions:
  https://developer.apple.com/documentation/safariservices/managing-safari-web-extension-permissions

## Decision rule

When choosing the next change, prefer the smallest shared primitive that:

1. protects an existing trust boundary;
2. solves a repeatedly observed user job;
3. produces evidence a reviewer can inspect;
4. preserves Centaur's local-first and fail-closed behavior; and
5. leaves specialized domain behavior in prompts or recipes until code is justified.
