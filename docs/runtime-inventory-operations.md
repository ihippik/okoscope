# Runtime inventory operations

Application runtime inventory is an additive projection of accepted runtime events. Raw events, runtime groups, release summaries, and notifications remain the source evidence and continue working when inventory reads are disabled.

## Staged rollout

1. Apply database migration 8 and confirm `/ready` reports the required schema.
2. Deploy the server with live ingestion projection enabled. New events update inventory in the same transaction as raw storage and grouping.
3. Backfill one Project, or one Application for a smaller canary, in bounded batches:

   ```sh
   okoscope-server --database-url "$OKOSCOPE_DATABASE_URL" inventory-backfill \
     --organization-id ORGANIZATION_UUID \
     --project-id PROJECT_UUID \
     --application-id APPLICATION_UUID \
     --identity-version 1 \
     --batch-size 500 \
     --throttle-ms 25
   ```

4. Run reconciliation for every backfilled Application:

   ```sh
   okoscope-server --database-url "$OKOSCOPE_DATABASE_URL" inventory-reconcile \
     --organization-id ORGANIZATION_UUID \
     --project-id PROJECT_UUID \
     --application-id APPLICATION_UUID \
     --identity-version 1
   ```

5. Enable inventory API and UI traffic only after reconciliation exits successfully.

The backfill snapshots an upper event identifier, processes bounded ordered batches, and skips existing versioned event memberships. Repeating or resuming the command is safe. It creates no runtime-group first-seen outbox work and therefore cannot deliver historical first-seen notifications.

## Readiness and monitoring

Monitor the following metrics:

- `okoscope_inventory_projection_events_total` and `okoscope_inventory_projection_skips_total`;
- `okoscope_inventory_items_created_total`;
- `okoscope_inventory_projection_duration_microseconds_total`;
- `okoscope_inventory_backfill_scanned`, `projected`, and `skipped`;
- `okoscope_inventory_projection_freshness_seconds`;
- `okoscope_inventory_reconciliation_mismatches_total`;
- inventory query request and duration totals.

Use [`ops/queries/runtime-inventory.sql`](../ops/queries/runtime-inventory.sql) to inspect per-Application projection totals, missing memberships, and kind cardinality without selecting event payloads.

## Rollback and rebuild

An older server image can run against migration 8 because the schema change is additive. To roll back the feature, stop inventory API traffic and deploy the older server; do not drop projection tables during an incident.

To rebuild one controlled tenant scope:

1. Stop inventory reads and live ingestion for the selected Application.
2. Export counts for diagnosis.
3. Delete the selected Application rows from `runtime_inventory_items`; cascading foreign keys remove its projection memberships, links, releases, and sightings without deleting source evidence.
4. Run `inventory-backfill` and `inventory-reconcile` for that Application.
5. Resume ingestion and inventory reads after reconciliation succeeds.

Never delete `runtime_events`, `runtime_event_groups`, their memberships, or release summaries as part of an inventory rebuild.
