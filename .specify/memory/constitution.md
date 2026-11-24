<!--
Sync Impact Report:
- Version change: 1.2.1 -> 1.3.0
- Modified principles:
  - I. Bilingual Communication & Documentation Policy -> Added normative rules for fixed English
    machine-readable schemas, JSON/CSV field names, audit logs, and CLI flags; expanded the
    Domain Carve-Out to cover CJK characters/glyphs, Unicode sequence examples, and localized UI
    control labels cited inside English engineering documents.
  - II. Test-Driven Development (NON-NEGOTIABLE) -> Mandated 1:1 task-level Red-before-Green
    ordering in `tasks.md` across core, CLI, and headless GUI verification layers.
  - III. Implementation Integrity & Zero Workarounds (NON-NEGOTIABLE) -> Required pinned
    official standards/lookup data (build-time UCD/UTS generation and embedded collation/normalization
    data without host-OS locale dependencies) and real temporary-directory filesystem tests.
  - IV. Evidence-Based Research & Adversarial Multi-Persona Verification (NON-NEGOTIABLE) ->
    Added cross-artifact schema and constrained-field traceability requirements plus empirical
    platform evidence reconciliation (verified screenshots and native OS API parity checks).
  - V. Scope Discipline & Non-Destructive Default (NON-NEGOTIABLE) -> Codified two-track
    remediation separation (`AutoFixable` vs. `ManualReview`), pre-flight collision/length/symlink
    validation, atomic non-overwriting rename with guarded SMB fallback, two-step rename rollback
    and orphaned-temp recovery, external reversible audit logging, and output sanitization gates.
- Updated sections:
  - Quality & Verification Standards -> Aligned verification gates with schema/field traceability,
    real-filesystem testing, and injection-safe output sanitization.
  - Development Workflow -> Clarified cross-artifact reconciliation before Red-Green-Refactor.
  - Governance -> Updated compliance review scope to cover specifications, plans, data models,
    contracts, task lists, and implementation diffs.
- Added sections:
  - None
- Removed sections:
  - None
- Follow-up TODOs:
  - None
-->
# nameChecker Constitution

## Core Principles

### I. Bilingual Communication & Documentation Policy

- All direct communication with the user—including questions, clarifications, status updates,
  analysis reports, and final answers—MUST be conducted in Korean.
- All internal reasoning, technical analysis, and committed project artifacts—including
  specifications, implementation plans, task lists, architecture documents, data models, contracts,
  docstrings, code comments, and this constitution—MUST be authored in English.
- Machine-readable contracts and identifiers—including CLI command/flag names, JSON/CSV report
  field names, schema enums, exit codes, and rename audit log keys—MUST use fixed English
  identifiers across all environments, while human-readable terminal reports, issue explanations,
  and GUI copy MUST default to Korean with an explicit English switch option.
- **Explicit Domain Carve-Out**: Verbatim user prompt quotes, multilingual CJK/Unicode test
  fixtures and character/sequence examples, localized `i18n` message catalogs, and localized UI
  control labels cited inside English engineering artifacts MAY contain non-English text where
  required by functional specifications.

Rationale: Separating user communication (Korean) from engineering documentation and machine
contracts (English) maximizes clarity and schema stability without blocking CJK test fixtures,
character examples, or localized UI labels.

### II. Test-Driven Development (NON-NEGOTIABLE)

- Test-Driven Development (TDD) MUST be the default methodology for all implementation work across
  core library, CLI, and GUI layers.
- Production implementation code MUST NOT be written without accompanying automated test code.
- Task breakdowns (`tasks.md`) MUST enforce explicit task-level Red-before-Green ordering: every
  production implementation task MUST be preceded by a dedicated failing automated test task
  covering the target behavior (unit, corpus, contract, integration, or headless UI tests).
- Every feature, behavioral change, or bug fix MUST strictly execute the Red-Green-Refactor cycle:
  1. **Red**: Write automated unit, corpus, contract, headless UI, or integration tests capturing
     the requirement and observe them fail against the codebase before writing production logic.
  2. **Green**: Implement the minimal genuine production logic required to make the failing tests
     pass.
  3. **Refactor**: Clean up duplication and structure while keeping the entire workspace test suite
     green.

Rationale: Enforcing 1:1 test-before-implementation task ordering guarantees falsifiable coverage
across every crate, prevents untested edge cases, and catches regressions immediately.

### III. Implementation Integrity & Zero Workarounds (NON-NEGOTIABLE)

- Production code MUST NOT hardcode expected test outputs, branch on specific test input literals,
  or substitute mockups, stubs, or fake results for real domain logic.
- Domain classification, Unicode normalization/decomposition, character inspection, and cross-platform
  collation MUST be driven deterministically by pinned official standards data (such as build-time
  generated UCD/UTS/unihan/KS X 1001 lookup tables and embedded collation/normalization data)
  rather than ad-hoc string heuristics or unpinned host-OS locale settings.
- Contributors and agents MUST NOT introduce workaround logic, weaken assertions, skip failing
  tests, or silently swallow errors solely to make a test suite pass.
- Test suites MUST use triangulated assertions—covering positive cases, edge/boundary cases, and
  explicit negative cases—and filesystem rename/collision tests MUST run against real temporary
  directories (`tempfile`) rather than mocked filesystems.

Rationale: Deterministic, standards-backed production logic paired with real-filesystem triangulated
tests eliminates brittle shortcuts and guarantees identical results across macOS, Windows, Linux,
and Synology NAS environments.

### IV. Evidence-Based Research & Adversarial Multi-Persona Verification (NON-NEGOTIABLE)

- Agents and contributors MUST NOT rely on solo guesswork, memory alone, or unverified third-party
  summaries when making domain, architectural, standards, OS, filesystem, or library API decisions.
- Every specification, plan, data model, contract, task breakdown, and technical claim MUST be
  verified using research tools against authoritative sources (official Unicode/UCD/UTS/CLDR
  standards, official OS and crate documentation, peer-reviewed literature, and empirical
  evidence such as verified OS screenshots and native OS API parity tests).
- All planning artifacts (`spec.md`, `plan.md`, `research.md`, `data-model.md`, `contracts/`,
  `quickstart.md`, and `tasks.md`) MUST maintain strict cross-artifact traceability: every enum
  variant, constrained entity field, exit code, and UI workflow state MUST match identically across
  specification, schema, data model, and tasks before implementation starts.
- Before finalizing any design or milestone, agents MUST conduct an adversarial multi-persona
  audit (standards specialist, systems/filesystem engineer, security auditor, performance/UX critic)
  across 12 mandatory criteria: **consistency, contradictions, conflicts, errors, performance,
  security, currency, usability, feasibility, suitability, integrity, and accuracy**.

Rationale: Cross-verifying official standards, empirical platform behavior, and inter-document
schema contracts before coding prevents silent drift, OS-specific bugs, and untestable requirements.

### V. Scope Discipline & Non-Destructive Default (NON-NEGOTIABLE)

- Both the agent workflow and the delivered software MUST default to non-destructive, read-only
  inspection (`max_depth = 1` single-folder scope in v1) unless a state mutation is explicitly
  requested and confirmed by the user.
- When the user places a hold on implementation or invokes a read-only analysis command, agents
  MUST NOT create build/source files, modify source files, or advance to implementation without
  explicit user approval.
- Deterministic `AutoFixable` remediation MUST be strictly separated from `ManualReview` items so
  ambiguous encodings, PUA mappings, confusables, punctuation variants, or hazards are NEVER
  auto-converted without explicit manual user edits or confirmation.
- Every filesystem mutation MUST satisfy pre-flight safety gates—checking sibling collisions,
  intra-batch duplicate targets, OS single-component length limits, and non-symlink constraints;
  trying atomic non-overwriting OS rename primitives first with guarded network/SMB fallback only
  on unsupported-flag errors; using two-step temporary renames with bounded lock-retry, automatic
  rollback, post-rename disk verification, and orphaned-temp detection; and writing a reversible
  external audit log (`file_id` + metadata fallback) for path-traversal-validated `undo`.
- Human-facing terminal/GUI outputs and structured CSV exports MUST sanitize or escape control
  characters, ANSI/OSC sequences, Bidi override characters, and spreadsheet formula prefixes to
  prevent injection and visual spoofing.

Rationale: Strict separation between read-only analysis and guarded, auditable mutations protects
user data and shared NAS volumes from accidental corruption, silent overwrites, and visual/shell
injection attacks.

## Quality & Verification Standards

Every change MUST satisfy the following verification gates before it is considered complete:

- **Falsifiable TDD Gate**: Every implementation task MUST be preceded by a failing automated test
  and verified green across the full workspace test suite (`cargo test --workspace`) without skipped
  or weakened assertions.
- **Deterministic Standards & Integrity Gate**: Production paths MUST use pinned official standards
  data and real-filesystem tests, rejecting hardcoded test answers, production mocks/stubs, or
  silent error suppression.
- **Cross-Artifact Schema & Reference Audit Gate**: Specifications, plans, data models, JSON/CLI
  contracts, and task lists MUST cite verified official/empirical references and stay 100% consistent
  across all constrained fields and enum values.
- **Non-Destructive & Injection-Safety Gate**: Filesystem mutations MUST require explicit confirmation,
  pre-flight collision/length/symlink validation, and reversible audit logging, and rendered outputs
  MUST escape control/ANSI/Bidi/formula sequences.
- **Bilingual Compliance Gate**: All committed Markdown documentation, docstrings, code comments,
  and machine contract keys MUST be written in English (subject only to Principle I's explicit
  domain carve-out), while all direct user communication MUST be in Korean.

## Development Workflow

All contributors and agents MUST adhere to this execution flow:

1. **Research & Specify in English**: Verify domain, OS, filesystem, and Unicode facts via
   authoritative references and empirical evidence; author specifications, data models, contracts,
   plans, and TDD-ordered task lists in English while communicating with the user in Korean.
2. **Adversarial Multi-Persona Audit**: Challenge all planning artifacts across Principle IV's 12
   criteria and verify 100% requirement-to-task and constrained-field traceability; apply revisions
   only as approved by the user.
3. **Red**: Write triangulated automated tests (positive, boundary, negative) and observe them
   fail before writing production code.
4. **Green**: Implement minimal, genuine, standards-compliant production logic until all tests pass.
5. **Refactor, Verify & Report**: Clean up code, verify lint/formatting (`cargo clippy`, `cargo fmt`),
   re-run the full workspace suite, and report verified outcomes to the user in Korean.

## Governance

This constitution supersedes all other project practices and conventions. The key words `MUST`,
`MUST NOT`, `REQUIRED`, `SHALL`, `SHALL NOT`, `SHOULD`, `SHOULD NOT`, `RECOMMENDED`, `MAY`, and
`OPTIONAL` in this document are to be interpreted as described in BCP 14 (RFC 2119 and RFC 8174)
when, and only when, they appear in all capitals.

- **Amendment Procedure**: Any amendment to this constitution MUST include a documented rationale,
  a Sync Impact Report comment at the top of the constitution file summarizing version and scope
  changes for human review, an updated `Last Amended` date in ISO `YYYY-MM-DD` format, and a
  semantic version increment.
- **Versioning Policy**:
  - `MAJOR`: Backward-incompatible removals or redefinitions of core principles.
  - `MINOR`: Addition of new principles/sections or material expansion of existing governance rules.
  - `PATCH`: Clarifications, wording improvements, or non-semantic refinements.
- **Compliance Review**: Every specification, research artifact, data model, contract, implementation
  plan, task list, analysis report, and implementation review MUST verify compliance with Principles
  I–V before approval.

**Version**: 1.3.0 | **Ratified**: 2026-11-18 | **Last Amended**: 2026-11-19
