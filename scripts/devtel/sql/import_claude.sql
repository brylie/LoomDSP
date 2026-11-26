-- Claude Code transcripts into claude_records. $FILES is replaced with a
-- list of paths. Only metadata and command lines are kept (ADR-0048).
insert or ignore into claude_records
with raw as (
  select * from read_json($FILES,
    format = 'newline_delimited', union_by_name = true, ignore_errors = true, filename = true,
    columns = {
      type: 'VARCHAR', subtype: 'VARCHAR', uuid: 'VARCHAR', parentUuid: 'VARCHAR',
      sessionId: 'VARCHAR', agentId: 'VARCHAR', isSidechain: 'BOOLEAN', isMeta: 'BOOLEAN',
      timestamp: 'TIMESTAMPTZ', cwd: 'VARCHAR', gitBranch: 'VARCHAR', version: 'VARCHAR',
      message: 'JSON', toolUseResult: 'JSON'
    })
  where uuid is not null and sessionId is not null
),
parts as (
  select *,
    case when json_type(message -> 'content') = 'ARRAY'
      then from_json(message -> 'content', '["JSON"]') else [] end as items
  from raw
)
select
  sessionId, uuid, parentUuid, agentId, type, subtype, timestamp, cwd, gitBranch, version,
  coalesce(isSidechain, false),
  message ->> 'model',
  message ->> 'stop_reason',
  (message -> 'usage' ->> 'input_tokens')::bigint,
  (message -> 'usage' ->> 'output_tokens')::bigint,
  (message -> 'usage' ->> 'cache_read_input_tokens')::bigint,
  (message -> 'usage' ->> 'cache_creation_input_tokens')::bigint,
  type = 'user' and not coalesce(isMeta, false) and json_type(message -> 'content') = 'VARCHAR',
  case when type = 'user' and json_type(message -> 'content') = 'VARCHAR'
    then length(message ->> 'content') end,
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
  [{
    'id': c ->> 'tool_use_id',
    'is_error': coalesce((c ->> 'is_error')::boolean, false),
    'interrupted': coalesce((toolUseResult ->> 'interrupted')::boolean, false)
  } for c in items if c ->> 'type' = 'tool_result'],
  filename
from parts;
