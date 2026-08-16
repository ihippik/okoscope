SELECT
  id,
  event_kind,
  semantic_summary,
  status,
  first_seen_at,
  last_seen_at,
  occurrence_count,
  namespace,
  workload_kind,
  workload_name
FROM runtime_event_groups
ORDER BY last_seen_at DESC, id DESC
LIMIT 100;

SELECT
  count(*) AS group_count,
  coalesce(sum(occurrence_count), 0)::bigint AS grouped_occurrences,
  (SELECT count(*) FROM runtime_event_group_memberships) AS memberships,
  (SELECT count(*) FROM runtime_events e
   WHERE NOT EXISTS (
     SELECT 1 FROM runtime_event_group_memberships m
     WHERE m.event_id = e.id AND m.fingerprint_version = 1
   )) AS ungrouped_events,
  (SELECT count(*) FROM outbox_messages WHERE processed_at IS NULL) AS pending_outbox;
