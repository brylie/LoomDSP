-- Claude Code transcripts into claude_records. $FILES is replaced with a
-- list of paths. Every record type is read, but only metadata and command
-- lines are kept (ADR-0048).
insert or ignore into claude_records
with raw as (
  select json as j, filename
  from read_json_objects($FILES, format = 'newline_delimited', ignore_errors = true, filename = true)
  where json ->> 'sessionId' is not null
),
parts as (
  select *,
    j ->> 'type' as type,
    j -> 'message' as message,
    j -> 'toolUseResult' as result,
    case when json_type(j -> 'message' -> 'content') = 'ARRAY'
      then from_json(j -> 'message' -> 'content', '["JSON"]') else [] end as items
  from raw
)
select
  j ->> 'sessionId',
  coalesce(j ->> 'uuid', md5(j::varchar)),
  j ->> 'uuid', j ->> 'parentUuid', j ->> 'agentId', type, j ->> 'subtype',
  coalesce((j ->> 'timestamp')::timestamptz,
    case when j ->> 'startTime' is not null then to_timestamp((j ->> 'startTime')::double / 1000) end),
  j ->> 'cwd', j ->> 'gitBranch', j ->> 'version', j ->> 'entrypoint',
  coalesce((j ->> 'isSidechain')::boolean, false),
  message ->> 'id', j ->> 'requestId', message ->> 'model', j ->> 'requestedModel', j ->> 'advisorModel',
  coalesce(j ->> 'perTurnEffort', j ->> 'effort'),
  (j ->> 'thinkingDurationMs')::bigint,
  message ->> 'stop_reason',
  case when (j ->> 'isApiErrorMessage')::boolean
    then coalesce(j ->> 'apiErrorType', j ->> 'apiErrorStatus', 'error') end,
  (message -> 'usage' ->> 'input_tokens')::bigint,
  (message -> 'usage' ->> 'output_tokens')::bigint,
  (message -> 'usage' ->> 'cache_read_input_tokens')::bigint,
  (message -> 'usage' ->> 'cache_creation_input_tokens')::bigint,
  type = 'user' and not coalesce((j ->> 'isMeta')::boolean, false) and json_type(message -> 'content') = 'VARCHAR',
  case when type = 'user' and json_type(message -> 'content') = 'VARCHAR' then length(message ->> 'content') end,
  [{
    'id': c ->> 'id', 'name': c ->> 'name',
    'command': left(c -> 'input' ->> 'command', 2000),
    'description': left(c -> 'input' ->> 'description', 300),
    'file_path': coalesce(c -> 'input' ->> 'file_path', c -> 'input' ->> 'path', c -> 'input' ->> 'notebook_path'),
    'pattern': left(c -> 'input' ->> 'pattern', 300),
    'subagent_type': c -> 'input' ->> 'subagent_type',
    'model': c -> 'input' ->> 'model',
    'run_in_background': (c -> 'input' ->> 'run_in_background')::boolean,
    'skill': c -> 'input' ->> 'skill',
    'url': c -> 'input' ->> 'url'
  } for c in items if c ->> 'type' = 'tool_use'],
  [{'id': c ->> 'tool_use_id', 'is_error': coalesce((c ->> 'is_error')::boolean, false)}
    for c in items if c ->> 'type' = 'tool_result'],
  case when json_type(result) = 'OBJECT' then (result ->> 'interrupted')::boolean end,
  case when json_type(result) = 'OBJECT' then (result -> 'gitOperation')::varchar end,
  case when json_type(result) = 'OBJECT' then result ->> 'agentId' end,
  case when json_type(result) = 'OBJECT' then result ->> 'resolvedModel' end,
  (j ->> 'prNumber')::integer, j ->> 'prRepository',
  left(coalesce(j ->> 'customTitle', j ->> 'aiTitle', j ->> 'agentName'), 200),
  (j ->> 'totalCostUSD')::double, (j ->> 'totalAPIDuration')::bigint, (j ->> 'totalToolDuration')::bigint,
  (j ->> 'totalLinesAdded')::bigint, (j ->> 'totalLinesRemoved')::bigint,
  j -> 'modelUsage',
  filename
from parts
-- Records that are only Claude Code's own bookkeeping, and large: the
-- request bodies, snapshots, and data-flow logs.
where type not in ('api-request', 'api-request-blob', 'api-request-shape', 'file-history-snapshot',
  'file-history-delta', 'data-flow-records', 'data-flow-incomplete', 'attachment', 'atis-latch',
  'bridge-session', 'last-prompt', 'queue-operation', 'tools-used', 'relocated', 'session-owner');
