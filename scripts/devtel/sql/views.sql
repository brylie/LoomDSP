-- Views over the devtel archive (docs/dev/devtel.md). Every import replaces
-- them, so changing a view here changes how all of history reads.

-- What kind of work a tool call is. Order matters: `git commit` runs the
-- pre-commit hooks, so it counts as committing even when it also tests.
create or replace macro devtel_category(tool, command) as case
  when tool in ('AskUserQuestion', 'ExitPlanMode') then 'wait: person'
  when tool in ('Agent', 'Task', 'SendMessage') then 'agents'
  when tool in ('Edit', 'Write', 'NotebookEdit', 'MultiEdit') then 'edit'
  when tool in ('Read', 'Grep', 'Glob', 'LS') then 'read'
  when tool in ('WebFetch', 'WebSearch') then 'web'
  when tool like 'mcp\_\_%' escape '\' then 'mcp'
  when tool <> 'Bash' then 'other'
  when regexp_matches(command, '\bgit\s+commit\b') then 'git commit'
  when regexp_matches(command, '\bgit\s+push\b') then 'git push'
  when regexp_matches(command, '\b(cargo\s+(test|nextest|llvm-cov)|mise\s+run\s+(test|coverage|doctest))\b') then 'test'
  when regexp_matches(command, '\b(prek\s+run|mise\s+run\s+check|cargo\s+(clippy|fmt|deny|doc)|typos)\b') then 'check'
  when regexp_matches(command, '\bcargo\s+(build|run|check|bench)\b') then 'build'
  when regexp_matches(command, '^\s*(cd\s+\S+\s*&&\s*)?gh\s') then 'github'
  when regexp_matches(command, '^\s*(cd\s+\S+\s*&&\s*)?git\s') then 'git'
  when regexp_matches(command, '\bmise\s+run\b') then 'mise'
  else 'shell'
end;

-- A command with the parts that do not change what it checks removed: a
-- leading `cd`, output redirection, and a trailing head, tail, or grep.
create or replace macro devtel_normalize(command) as trim(regexp_replace(regexp_replace(
  coalesce(command, ''),
  '^\s*cd\s+[^&;]+(&&|;)\s*', ''),
  '\s*(\d?>\s*&?\s*\S+|\|\s*(tail|head|grep|wc)\b.*)', '', 'g'));

create or replace macro devtel_span_id(name) as left(md5(name), 16);

-- One row per tool call, with its result.
create or replace view devtel_tools as
with calls as (
  select r.session_id, r.agent_id, r.ts as start_ts, r.git_branch, r.cwd, r.model, unnest(r.tool_calls) as c
  from claude_records r where r.type = 'assistant' and len(r.tool_calls) > 0
),
result_items as (
  select r.session_id, r.ts, unnest(r.tool_results) as t
  from claude_records r where r.type = 'user' and len(r.tool_results) > 0
),
-- A forked session can repeat a result, so take the first of each.
results as (
  select session_id, t.id as tool_use_id, min(ts) as end_ts,
    bool_or(t.is_error) as error, bool_or(t.interrupted) as interrupted
  from result_items group by all
)
select
  calls.session_id, calls.agent_id, calls.start_ts,
  coalesce(results.end_ts, calls.start_ts) as end_ts,
  c.id as tool_use_id, c.name as tool, c.command, c.description, c.file_path,
  c.subagent_type, c.model as requested_model, calls.model, calls.git_branch, calls.cwd,
  coalesce(results.error, false) as error,
  coalesce(results.interrupted, false) as interrupted,
  devtel_category(c.name, c.command) as category,
  devtel_normalize(c.command) as normalized
from calls left join results
  on results.session_id = calls.session_id and results.tool_use_id = c.id;

-- Every span, from every source, in one OpenTelemetry shape. Claude Code
-- sessions are traces whose ID is the session UUID; subagents and tool
-- calls hang under them. A span from the spool whose parent is a session's
-- root is moved under the Bash call it ran inside, found by time.
create or replace view devtel_spans as
with sessions as (
  select session_id, min(ts) as start_ts, max(ts) as end_ts,
    mode(git_branch) as git_branch, mode(model) filter (where model is not null) as model
  from claude_records group by session_id
),
roots as (
  select replace(session_id, '-', '') as trace_id, devtel_span_id('session:' || session_id) as span_id,
    null::varchar as parent_span_id, 'claude session' as name, 'claude-code' as service,
    start_ts, end_ts, false as error, session_id, null::varchar as agent_id, 'session' as category,
    map_from_entries([('gen_ai.conversation.id', session_id), ('vcs.ref.head.name', git_branch),
      ('gen_ai.request.model', model)]) as attributes
  from sessions
),
agent_times as (
  select session_id, agent_id, min(ts) as start_ts, max(ts) as end_ts, mode(model) as model
  from claude_records where agent_id is not null group by all
),
agents as (
  select replace(t.session_id, '-', ''), devtel_span_id('agent:' || t.agent_id),
    coalesce(devtel_span_id('tool:' || a.tool_use_id), devtel_span_id('session:' || t.session_id)),
    'invoke_agent ' || coalesce(a.agent_type, 'agent'), 'claude-code',
    t.start_ts, t.end_ts, false, t.session_id, t.agent_id, 'agent',
    map_from_entries([('gen_ai.operation.name', 'invoke_agent'), ('gen_ai.agent.id', t.agent_id),
      ('gen_ai.agent.name', a.agent_type), ('gen_ai.request.model', coalesce(a.model, t.model)),
      ('gen_ai.conversation.id', t.session_id), ('loom.devtel.agent.description', a.description)])
  from agent_times t left join claude_agents a using (session_id, agent_id)
),
tools as (
  select replace(session_id, '-', ''), devtel_span_id('tool:' || tool_use_id),
    case when agent_id is null then devtel_span_id('session:' || session_id) else devtel_span_id('agent:' || agent_id) end,
    'execute_tool ' || tool, 'claude-code', start_ts, end_ts, error, session_id, agent_id, category,
    map_from_entries(list_filter([
      ('gen_ai.operation.name', 'execute_tool'), ('gen_ai.tool.name', tool), ('gen_ai.tool.call.id', tool_use_id),
      ('gen_ai.conversation.id', session_id), ('gen_ai.agent.id', agent_id), ('gen_ai.request.model', model),
      ('process.command_line', command), ('loom.devtel.file_path', file_path),
      ('loom.devtel.category', category), ('vcs.ref.head.name', git_branch)], lambda x: x[2] is not null))
  from devtel_tools
),
claude as (
  select * from roots union all select * from agents union all select * from tools
),
spool as (
  select s.trace_id, s.span_id, s.parent_span_id, s.name, s.service,
    to_timestamp(s.start_ns::double / 1e9) as start_ts, to_timestamp(s.end_ns::double / 1e9) as end_ts,
    s.error, nullif(s.attributes['gen_ai.conversation.id'], '') as session_id, null::varchar as agent_id,
    case when s.service = 'git' then 'git event' when s.attributes['loom.devtel.hook_stage'] <> '' then 'hook'
      else 'mise task' end as category,
    s.attributes
  from spool_spans s
),
moved as (
  select s.* replace (coalesce(b.span_id, s.parent_span_id) as parent_span_id)
  from spool s left join (select * from claude where name = 'execute_tool Bash') b
    on b.trace_id = s.trace_id and s.parent_span_id = devtel_span_id('session:' || b.session_id)
    and b.start_ts <= s.start_ts and b.end_ts >= s.end_ts
  qualify row_number() over (partition by s.trace_id, s.span_id order by b.end_ts - b.start_ts) = 1
)
select *, epoch_ms(end_ts) - epoch_ms(start_ts) as duration_ms from claude
union all
select *, epoch_ms(end_ts) - epoch_ms(start_ts) from moved;

-- One row per Claude Code session: size, cost in time and tokens, and what
-- it changed. Commits come from the git hooks, so only sessions after
-- devtel was installed have them.
create or replace view devtel_sessions as
with records as (
  select session_id, min(ts) as start_ts, max(ts) as end_ts, mode(git_branch) as git_branch,
    count(*) filter (where is_prompt) as prompts,
    sum(input_tokens) as input_tokens, sum(output_tokens) as output_tokens,
    sum(cache_read_tokens) as cache_read_tokens, sum(cache_write_tokens) as cache_write_tokens,
    string_agg(distinct model, ', ') filter (where model is not null and model not like '<%') as models
  from claude_records group by session_id
),
tools as (
  select session_id, count(*) as tool_calls,
    count(distinct agent_id) as subagents,
    round(sum(epoch_ms(end_ts) - epoch_ms(start_ts)) filter (where category = 'wait: person') / 60000.0, 1) as waiting_min,
    round(sum(epoch_ms(end_ts) - epoch_ms(start_ts)) filter (where category in ('test', 'check', 'build', 'git commit', 'git push')) / 60000.0, 1) as checking_min
  from devtel_tools group by session_id
),
commits as (
  select attributes['gen_ai.conversation.id'] as session_id, count(*) as commits,
    sum(attributes['loom.devtel.commit.insertions']::bigint) as insertions,
    sum(attributes['loom.devtel.commit.deletions']::bigint) as deletions
  from spool_spans where attributes['loom.devtel.git.event'] = 'post-commit' group by 1
)
select records.*, round((epoch_ms(end_ts) - epoch_ms(start_ts)) / 60000.0, 1) as wall_min,
  tools.* exclude (session_id), commits.* exclude (session_id)
from records left join tools using (session_id) left join commits using (session_id);

-- Checks run again with nothing changed in between. Two sources:
-- "same tree": a mise task step that ran on the same working tree as an
--   earlier run of the same step (exact, from the task wrapper).
-- "no edit": an agent ran the same check command again with no Edit or
--   Write in between (an estimate: code can also change through git or a
--   shell command, so this is an upper bound).
create or replace view devtel_reruns as
with task_runs as (
  select attributes['mise.task.name'] as task, attributes['process.command_line'] as step,
    attributes['loom.devtel.tree'] as tree, nullif(attributes['gen_ai.conversation.id'], '') as session_id,
    to_timestamp(start_ns::double / 1e9) as start_ts, (end_ns - start_ns) / 1e6 as ms,
    attributes['process.exit.code'] as exit_code
  from spool_spans where service = 'mise' and coalesce(attributes['loom.devtel.tree'], '') <> ''
),
same_tree as (
  select 'same tree' as kind, session_id, task || ': ' || step as command, start_ts, ms,
    row_number() over (partition by task, step, tree, exit_code order by start_ts) as run
  from task_runs
),
ordered as (
  select *, sum(case when category = 'edit' then 1 else 0 end)
      over (partition by session_id, agent_id order by start_ts rows unbounded preceding) as edits
  from devtel_tools
),
no_edit as (
  select 'no edit' as kind, session_id, normalized as command, start_ts,
    epoch_ms(end_ts) - epoch_ms(start_ts) as ms,
    row_number() over (partition by session_id, agent_id, edits, normalized order by start_ts) as run
  from ordered where category in ('test', 'check', 'build') and not interrupted
)
select * from same_tree where run > 1
union all
select * from no_edit where run > 1;

-- Possible loops: the same command three or more times within ten tool
-- calls of one agent.
create or replace view devtel_loops as
with numbered as (
  select *, row_number() over (partition by session_id, agent_id order by start_ts) as seq
  from devtel_tools where tool = 'Bash' and normalized <> ''
),
windowed as (
  select *, count(*) over (partition by session_id, agent_id, normalized order by seq range between 10 preceding and current row) as in_window
  from numbered
)
select session_id, agent_id, normalized as command, max(in_window) as times_in_ten_calls,
  count(*) as times_in_session, round(sum(epoch_ms(end_ts) - epoch_ms(start_ts)) / 60000.0, 1) as minutes,
  min(start_ts) as first_ts
from windowed group by all having max(in_window) >= 3;
