# Frontend handoff: application runtime inventory

The Web UI is developed separately. This document defines the acceptance boundary for consuming the versioned runtime inventory API in `openapi/okoscope-v1.yaml`. Fixture data is available in [`fixtures/runtime-inventory.json`](fixtures/runtime-inventory.json).

## Application inventory route

Add an Application-level **Runtime inventory** view. Keep the active Project, Application, release, cluster, namespace, workload, container, and observation-window scope visible whenever filters are applied.

The initial view requests:

- `GET .../runtime-inventory/summary` for aggregate cards;
- `GET .../runtime-inventory?kind=...` for a shared paginated list;
- list filters exactly as described by OpenAPI.

Render four tabs: Processes, Destinations, Domains, and Syscalls. Each row shows its typed safe identity, first and last observation, occurrence count, and bounded release, cluster, namespace, workload, Pod, container, and runtime-group counts. Do not infer totals from the current page.

## Item detail

Selecting an item opens a detail route or drawer using the evidence links returned by the item endpoint. Present these independently paginated collections:

- release evidence;
- Kubernetes sightings;
- contributing runtime groups;
- raw occurrences.

Do not imply process → DNS → connection causality. Links between these behaviors are outside this milestone.

## Release wording

Use these exact meanings:

- `observed`: trusted attributed occurrences support the relation;
- `not_observed`: the release has other attributed evidence, but this item was not seen in that evidence;
- `unknown`: no trusted attributed evidence is available for evaluation.

Never relabel `not_observed` as “absent”, “removed”, or “safe”. Show `release_evidence_count` and the occurrence time bounds when present.

## Pagination and bounds

- Treat every cursor as opaque, including UUID-shaped cursors.
- Never request a limit greater than 200.
- Replace rather than concatenate results when the user changes scope, filters, identity version, or search.
- Support empty first pages and an empty terminal page.
- Do not embed or preload every Pod, release, group, or occurrence from the list view.

## Display safety

All executable, command, domain, namespace, workload, Pod, and container values are observed untrusted text. Render them through text nodes only. Do not use raw HTML, convert values to links automatically, interpret Markdown, or execute URL-like strings. DNS names are evidence, not navigation targets.

Use the `unsafe_display_text` fixture to verify inert rendering. No fixture string may create an element, navigation, event handler, or script execution.

## Acceptance checklist

- Overview cards use the summary response and include all four kinds.
- Each tab uses the typed semantic identity appropriate to its kind.
- Active scope and filters remain visible and shareable without exposing credentials.
- Search is debounced and limited to 200 characters.
- Item detail exposes release, sighting, group, and occurrence evidence navigation.
- All three release states use evidence-qualified wording.
- Loading, empty, invalid-cursor, unauthorized, not-found, and server-error states are distinct.
- `next_cursor` is treated as opaque and pages contain at most 200 items.
- Markup-like observed values remain inert in list, detail, filters, tooltips, and copied text previews.
