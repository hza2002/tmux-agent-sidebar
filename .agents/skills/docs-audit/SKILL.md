---
name: docs-audit
description: Audit this repository's maintained documentation against current source code and report concrete drift. Use for documentation consistency, stale paths or contracts, and implementation-plan status. Update files only when the user explicitly asks.
---

# Documentation Audit

Find durable documentation that contradicts the current repository. Historical
plans record intent and are not required to match the final implementation.

## Scope

Use the user's explicit file scope when provided. Otherwise inspect maintained
references under `docs/` and load historical material under `docs/superpowers/`
only when a current document depends on it.

Classify documents before judging drift:

- **Reference:** current architecture, state, commands, or maintenance policy.
- **Decision/design:** intent plus compatibility assumptions.
- **Plan:** implementation checklist and completion evidence.

`README.md`, `CHANGELOG.md`, generated assets, and agent entrypoints are outside
the default scope unless the request names them.

## Method

1. Inventory Markdown with `rg --files docs -g '*.md'`.
2. Extract testable claims: paths, type/variant names, command lines, data flow,
   ownership, and cross-file contracts.
3. Locate the exact current implementation with targeted `rg` searches and read
   the relevant callers or consumers. Public declarations alone do not prove a
   documented behavior.
4. Distinguish real drift from an intentional historical difference.
5. Check plan items only when plan completion is in scope; do not convert old
   plans into current architecture documentation.

## Report

List findings by severity, each with:

- document path and line;
- contradictory source path and line;
- the concrete mismatch and impact;
- the smallest documentation correction.

Use high severity for a contradiction that would cause incorrect maintenance,
medium for missing current contract information, and low for stale completion
metadata. If no drift is found, state which documents and source surfaces were
checked.

Do not edit, delete, or rewrite documentation unless the current request
explicitly authorizes it.
