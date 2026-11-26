-- Spool files (OTLP JSON lines, one TracesData object per line) into
-- spool_spans. Attribute values are kept as text; cast them in queries.
insert or ignore into spool_spans
with lines as (
  select unnest(from_json(resourceSpans, '["JSON"]')) as rs
  from read_json($FILES, format = 'newline_delimited', ignore_errors = true, columns = {resourceSpans: 'JSON'})
),
scopes as (
  select rs -> 'resource' -> 'attributes' as resource, unnest(from_json(rs -> 'scopeSpans', '["JSON"]')) as ss
  from lines
),
spans as (
  select resource, unnest(from_json(ss -> 'spans', '["JSON"]')) as s from scopes
),
kv as (
  select *,
    map_from_entries([
      (a ->> 'key', coalesce(a -> 'value' ->> 'stringValue', a -> 'value' ->> 'intValue',
        a -> 'value' ->> 'boolValue', a -> 'value' ->> 'doubleValue'))
      for a in from_json(coalesce(s -> 'attributes', '[]'), '["JSON"]')]) as attrs,
    map_from_entries([
      (a ->> 'key', coalesce(a -> 'value' ->> 'stringValue', a -> 'value' ->> 'intValue',
        a -> 'value' ->> 'boolValue', a -> 'value' ->> 'doubleValue'))
      for a in from_json(coalesce(resource, '[]'), '["JSON"]')]) as res
  from spans
)
select
  s ->> 'traceId', s ->> 'spanId', nullif(s ->> 'parentSpanId', ''), s ->> 'name',
  res['service.name'],
  (s ->> 'startTimeUnixNano')::hugeint, (s ->> 'endTimeUnixNano')::hugeint,
  coalesce((s -> 'status' ->> 'code')::integer, 0) = 2,
  attrs, res
from kv
where s ->> 'traceId' is not null and s ->> 'spanId' is not null;
