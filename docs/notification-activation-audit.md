# Notification activation implementation audit

The existing notification subsystem already provides the data-plane guarantees required for production activation:

- migration 4 stores encrypted destinations, durable deliveries, bounded attempt history, leases, and uniqueness for each outbox/destination pair;
- `FOR UPDATE SKIP LOCKED`, conditional lease ownership, and the partial unique index support concurrent server replicas;
- shutdown stops the worker loop and the server waits for an in-progress cycle for a bounded interval;
- URL policy, signing, response truncation, retry classification, backfill suppression, destination tests, tenant-scoped APIs, request IDs, and group notification summaries have integration coverage;
- current metrics include materialization, claims, attempts, retries, failures, pending, in-flight, and expired leases.

The smallest production activation change is therefore configuration and operations work, not a replacement worker or database migration:

1. fail fast when enabled notification configuration is invalid;
2. provide a listener-free `notification-check` command that validates configuration and schema and reports only safe activation metadata;
3. make shutdown drain an explicit bounded setting;
4. expose worker activation, enabled destination count, due/retrying/terminal counts, oldest due age, cycle failures, and drain outcomes;
5. render disabled/enabled worker settings strictly into immutable deployment artifacts and provenance;
6. prove the existing delivery contract locally and in the reference cluster before leaving activation enabled.

Receiver outages remain metrics/log signals and do not make the API unready. Delivery remains disabled by default, and global disable preserves durable outbox and delivery rows.

The destination, one-time-secret, test-delivery, delivery-history, and group-summary APIs already use explicit serialized structures and tenant-scoped pagination. Worker health remains an operator metric rather than a bearer-authenticated Web API: the bounded state gauge is sufficient for self-hosted activation, while the frontend milestone treats a future typed health endpoint as optional and does not invent a client-side contract.
