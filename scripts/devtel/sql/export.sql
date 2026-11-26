-- Every span of each trace that had activity in the last $DAYS days, for
-- export as OTLP JSON. Whole traces, so a long session keeps its root span.
with recent as (
  select distinct trace_id from devtel_spans where end_ts >= now() - to_days($DAYS)
)
select service, trace_id, span_id, parent_span_id, name,
  (epoch_ns(start_ts))::varchar as start_ns, (epoch_ns(end_ts))::varchar as end_ns, error,
  to_json(map_concat(attributes, map_from_entries([('loom.devtel.category', category)])))::varchar as attributes
from devtel_spans semi join recent using (trace_id)
order by trace_id, start_ts;
