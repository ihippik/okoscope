# Runtime inventory performance acceptance

The executable acceptance benchmark is `crates/server/tests/inventory_benchmark.rs`. It projects 10,000 accepted events into 1,000 application-level process items across 200 Pods, two namespaces, two releases, and roughly seven days, then measures bounded list, item-evidence, Runtime Inventory Distribution, and Runtime Diff Summary queries.

Run it against an isolated PostgreSQL database:

```sh
DATABASE_URL=postgres://localhost/okoscope_inventory_benchmark \
  cargo test -p server --test inventory_benchmark -- --ignored --nocapture
```

Initial acceptance limits for a development PostgreSQL instance are:

| Operation | Cardinality | Limit |
|---|---:|---:|
| Transactional projection | 10,000 events, 1,000 items, 200 Pods | 60 seconds |
| Bounded inventory list | maximum 200 returned items | 2 seconds |
| Item evidence count | 10 memberships for one item | 2 seconds |

These are regression ceilings, not production SLOs. Production sizing must additionally test expected retention, concurrent agents, release cardinality, distinct Pods, filters, and pagination using sanitized synthetic data. API pages remain capped at 200 regardless of database size.

The benchmark prints measured projection, list, and detail durations. Record results with the server revision, PostgreSQL version, machine resources, and database settings when changing projection tables or indexes.

## Recorded baseline

On 2026-08-20, the expanded benchmark passed locally with PostgreSQL 17 using the repository debug test profile:

- transactional projection: 22,007 ms;
- bounded inventory list: 9 ms;
- item evidence count: less than 1 ms.
- distribution p50/p95/p99: 2/3/5 ms;
- diff summary p50/p95/p99: 3/4/6 ms;
- maximum aggregate response: 10,615 bytes.

The distribution plan used an index-only scan on `runtime_inventory_items_distribution_idx`. The isolated database used default PostgreSQL settings. These measurements establish a developer regression baseline only and do not replace deployment-specific load testing; diff tail latency remains the primary observed limitation.
