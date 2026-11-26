-- Subagent metadata files (agent-<id>.meta.json) into claude_agents. The
-- session and agent IDs come from the path:
-- <project>/<session>/subagents/agent-<id>.meta.json
insert or ignore into claude_agents
select
  regexp_extract(filename, '/([0-9a-f-]{36})/subagents/', 1),
  regexp_extract(filename, 'agent-([^/]+)\.meta\.json$', 1),
  agentType, left(description, 300), toolUseId, model, spawnDepth, filename
from read_json($FILES,
  format = 'auto', union_by_name = true, ignore_errors = true, filename = true,
  columns = {agentType: 'VARCHAR', description: 'VARCHAR', toolUseId: 'VARCHAR', model: 'VARCHAR', spawnDepth: 'INTEGER'})
where regexp_extract(filename, '/([0-9a-f-]{36})/subagents/', 1) <> '';
