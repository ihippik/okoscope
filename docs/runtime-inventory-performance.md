# Runtime inventory performance acceptance

The executable acceptance benchmark is `crates/server/tests/inventory_benchmark.rs`. It projects 1,000 accepted events into 100 application-level process items across 20 Pods and two namespaces, then runs representative bounded list and item-evidence queries.

Run it against an isolated PostgreSQL database:

```sh
DATABASE_URL=postgres://localhost/okoscope_inventory_benchmark \
  cargo test -p server --test inventory_benchmark -- --ignored --nocapture
```

Initial acceptance limits for a development PostgreSQL instance are:

| Operation | Cardinality | Limit |
|---|---:|---:|
| Transactional projection | 1,000 events, 100 items, 20 Pods | 60 seconds |
| Bounded inventory list | 100 of maximum 200 returned items | 2 seconds |
| Item evidence count | 10 memberships for one item | 2 seconds |

These are regression ceilings, not production SLOs. Production sizing must additionally test expected retention, concurrent agents, release cardinality, distinct Pods, filters, and pagination using sanitized synthetic data. API pages remain capped at 200 regardless of database size.

The benchmark prints measured projection, list, and detail durations. Record results with the server revision, PostgreSQL version, machine resources, and database settings when changing projection tables or indexes.

## Recorded baseline

On 2026-08-18, the benchmark passed locally with PostgreSQL 17 using the repository debug test profile:

- transactional projection: 1,273 ms;
- bounded inventory list: 3 ms;
- item evidence count: less than 1 ms.

The isolated database used default PostgreSQL settings. These measurements establish a developer regression baseline only and do not replace deployment-specific load testing.
