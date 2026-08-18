## Why

The completed runtime inventory backend is sufficient for raw UI integration, but its contract leaves filtered summary semantics, discoverable filter options, evidence-link trust, and frontend acceptance fixtures underspecified. Hardening these boundaries before the Web UI is implemented avoids misleading aggregate cards, unscalable free-text filters, unsafe navigation behavior, and frontend tests built against an incomplete snapshot.

## What Changes

- Make the inventory summary endpoint accept the same operational scope filters as the inventory list and return counts computed from that selected scope across all four inventory kinds.
- Add bounded, searchable, cursor-paginated facet APIs for cluster, namespace, workload kind, workload name, and container name, with counts and dependent filtering suitable for select/autocomplete controls.
- Keep release options on the existing Releases API while allowing release scope to constrain summary, list, and applicable facet results.
- Define evidence navigation as known scoped routes; returned navigation hints, if retained, MUST be relative allowlisted paths for the current Project, Application, and item and MUST NOT be treated as arbitrary fetch targets.
- Expand contract fixtures to cover item detail, sightings, groups, occurrences, pagination, all release states, standard error envelopes, and inert unsafe values in every observed display field.
- Update OpenAPI contract tests and the frontend handoff so snapshot synchronization and `schema.d.ts` regeneration are explicit release gates before UI implementation.
- Preserve existing runtime inventory identity, projection, ingestion, backfill, reconciliation, and evidence semantics.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-runtime-inventory`: Adds scoped summary semantics, bounded facet discovery, trusted evidence-route constraints, and complete frontend contract-fixture requirements to the capability introduced by the completed `application-runtime-inventory` change.

## Impact

- Extends runtime inventory API query parsing and projection queries without changing stored identity or projection tables unless new facet indexes are justified by query-plan verification.
- Adds versioned authenticated facet routes under the existing Project/Application runtime-inventory hierarchy.
- Updates `openapi/okoscope-v1.yaml`, route inventory tests, filter contract tests, and representative response fixtures.
- Updates the separate-Web-UI handoff and requires synchronization of `/Users/ihippik/WebstormProjects/okoscope-web/openapi/okoscope-v1.yaml` followed by regeneration of its typed schema before frontend feature work.
- Depends logically on the completed `application-runtime-inventory` change; that change must be archived before this delta is archived into the main specification.
