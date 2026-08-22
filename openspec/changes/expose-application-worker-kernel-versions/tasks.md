## 1. Database and Session Persistence

- [x] 1.1 Add a migration with nullable bounded `architecture` and `kernel_release` columns on `agents`, update `REQUIRED_MIGRATION`, and update every pinned migration-version assertion found with `rg "REQUIRED_MIGRATION|required_database_migration"`.
- [x] 1.2 Update session registration to normalize empty, whitespace-only, and recognized unknown platform values to null and persist usable values on insert and reconnect conflict updates.
- [x] 1.3 Add session tests covering initial persistence, null normalization, stable agent identity on reconnect, and platform metadata refresh after a reported kernel change.
- [x] 1.4 Compile the server migration test target and, when `DATABASE_URL` is available, run the ignored PostgreSQL migration suite covering fresh initialization, current-schema idempotency, failed-latest-migration readiness, and repair/retry behavior.

## 2. Application Worker API

- [x] 2.1 Add typed application-worker response, page, query, and opaque cursor models with bounded limits and deterministic `(last_observed_at, agent_id)` ordering.
- [x] 2.2 Implement the authenticated `/api/v1/projects/{project_id}/applications/{application_id}/workers` handler using tenant-safe ownership validation and runtime-event evidence grouped by agent.
- [x] 2.3 Return cluster context, worker identity, agent version, nullable architecture and kernel release, Application first/last observation timestamps, and agent last-seen timestamp without deriving an online or compatibility verdict.
- [x] 2.4 Add API integration tests for heterogeneous worker kernels, null platform metadata, exclusion without Application evidence, pagination boundaries, malformed inputs, and cross-project/cross-tenant not-found behavior.
- [x] 2.5 Inspect the scoped grouped query plan against representative data and add a supporting runtime-events index only if the measured plan requires one.

## 3. Published Contract and Frontend Handoff

- [x] 3.1 Add the worker endpoint, parameters, nullable bounded platform fields, concrete page schemas, errors, and unique operation identifier to `openapi/okoscope-v1.yaml`.
- [x] 3.2 Extend OpenAPI syntax, schema, authentication, operation-ID, and implementation route-coverage tests for the new endpoint.
- [x] 3.3 Add or update a frontend handoff fixture and documentation showing multiple workers, heterogeneous kernels, unavailable metadata, inert-text rendering, and cursor pagination.

## 4. Verification

- [x] 4.1 Run focused server session and application-worker API tests, then the broader server test suite.
- [x] 4.2 Validate the OpenSpec change and OpenAPI document and confirm the working tree contains only intended files for this change.
