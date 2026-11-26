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

-- A command with the parts that do not change what it checks removed:
-- leading `cd`, `git add`, environment assignments, `time`, `nice`, and
-- `mise exec --`; output redirection; and a trailing head, tail, grep, or wc.
create or replace macro devtel_normalize(command) as trim(regexp_replace(regexp_replace(regexp_replace(
  coalesce(command, ''),
  '^(\s*(cd\s+[^&;]+|git\s+add\s+[^&;]+)\s*(&&|;)\s*)+', ''),
  '^(\s*([A-Z_][A-Z0-9_]*=\S*|time|nice(\s+-n\s*-?\d+)?|mise\s+exec\s+--)\s+)+', ''),
  '\s*(\d?>\s*&?\s*\S+|\|\s*(tail|head|grep|wc)\b.*)', '', 'g'));

-- Whether a tool call may have changed the code. Agents edit through Bash
-- as often as through the Edit tool, so a Bash command counts when it runs
-- an in-place editor, a script on stdin, a redirect into a file, a git
-- command that changes the working tree, or a formatter.
create or replace macro devtel_may_write(tool, command) as
  tool in ('Edit', 'Write', 'NotebookEdit', 'MultiEdit')
  or (tool = 'Bash' and regexp_matches(coalesce(command, ''),
    '(sed\s+(-[a-zA-Z]*\s+)*-i|perl\s+-[a-z]*i|python3?\s+(-\s|-c\s|\S+\.py)|node\s|<<|\btee\s|[^0-9&>]>\s*[^&/\s>]|\b(mv|cp|rm|touch|patch)\s|git\s+(apply|checkout|switch|restore|rebase|merge|pull|reset|stash|cherry-pick|am|revert)\b|cargo\s+(fmt|fix|clippy\s.*--fix)|prek\s+run|mise\s+run\s+help-pages)'));

create or replace macro devtel_span_id(name) as left(md5(name), 16);

-- One row per tool call, with its result.
create or replace view devtel_tools as
with calls as (
  select r.session_id, r.agent_id, r.ts as start_ts, r.git_branch, r.cwd, r.model, unnest(r.tool_calls) as c
  from claude_records r where r.type = 'assistant' and len(r.tool_calls) > 0
),
result_items as (
  select r.session_id, r.ts, r.result_interrupted, r.result_git_operation, unnest(r.tool_results) as t
  from claude_records r where r.type = 'user' and len(r.tool_results) > 0
),
-- A forked session can repeat a result, so take the first of each.
results as (
  select session_id, t.id as tool_use_id, min(ts) as end_ts,
    bool_or(t.is_error) as error, bool_or(result_interrupted) as interrupted,
    max(result_git_operation) as git_operation
  from result_items group by all
)
select
  calls.session_id, calls.agent_id, calls.start_ts,
  coalesce(results.end_ts, calls.start_ts) as end_ts,
  c.id as tool_use_id, c.name as tool, c.command, c.description, c.file_path,
  c.subagent_type, c.model as requested_model, calls.model, calls.git_branch, calls.cwd,
  coalesce(results.error, false) as error,
  coalesce(results.interrupted, false) as interrupted,
  results.git_operation,
  devtel_category(c.name, c.command) as category,
  devtel_normalize(c.command) as normalized
from calls left join results
  on results.session_id = calls.session_id and results.tool_use_id = c.id;

-- One row per assistant message (an API response). Claude Code writes a
-- record per content block and repeats the usage on each, so tokens are
-- taken once per message here.
create or replace view devtel_turns as
select session_id, message_id, any_value(request_id) as request_id, any_value(agent_id) as agent_id,
  min(ts) as ts, any_value(model) as model, any_value(requested_model) as requested_model,
  any_value(advisor_model) as advisor_model, any_value(effort) as effort, max(thinking_ms) as thinking_ms,
  max(input_tokens) as input_tokens, max(output_tokens) as output_tokens,
  max(cache_read_tokens) as cache_read_tokens, max(cache_write_tokens) as cache_write_tokens,
  any_value(git_branch) as git_branch
from claude_records
where type = 'assistant' and message_id is not null and model is not null and model not like '<%'
group by session_id, message_id;

-- Pull requests a session opened or worked on, from Claude Code's own
-- pr-link records.
create or replace view devtel_prs as
select session_id, pr_repository, pr_number, min(ts) as first_ts
from claude_records where type = 'pr-link' and pr_number is not null group by all;

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
    case when s.service = 'git' then 'git event' when s.attributes['loom.devtel.in_git_hook'] = 'true' then 'hook'
      else 'check' end as category,
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
-- it changed. Cost and lines come from Claude Code's cost records; commits
-- come from the git hooks, so only sessions after devtel was installed
-- have them.
create or replace view devtel_sessions as
with records as (
  select session_id, min(ts) as start_ts, max(ts) as end_ts, mode(git_branch) as git_branch,
    any_value(entrypoint) filter (where entrypoint is not null) as entrypoint,
    last(title order by ts) filter (where title is not null) as title,
    count(*) filter (where is_prompt) as prompts,
    count(*) filter (where api_error is not null) as api_errors,
    count(*) filter (where subtype = 'compact_boundary') as compactions,
    max(cost_usd) as cost_usd, max(lines_added) as lines_added, max(lines_removed) as lines_removed
  from claude_records group by session_id
),
turns as (
  select session_id, count(*) as turns,
    string_agg(distinct model, ', ') as models,
    sum(output_tokens) as output_tokens,
    sum(input_tokens + coalesce(cache_read_tokens, 0) + coalesce(cache_write_tokens, 0)) as input_tokens
  from devtel_turns group by session_id
),
tools as (
  select session_id, count(*) as tool_calls,
    count(distinct agent_id) as subagents,
    round(sum(epoch_ms(end_ts) - epoch_ms(start_ts)) filter (where category = 'wait: person') / 60000.0, 1) as waiting_min,
    round(sum(epoch_ms(end_ts) - epoch_ms(start_ts)) filter (where category in ('test', 'check', 'build', 'git commit', 'git push')) / 60000.0, 1) as checking_min
  from devtel_tools group by session_id
),
prs as (
  select session_id, string_agg(distinct '#' || pr_number, ' ') as prs from devtel_prs group by session_id
),
commits as (
  select attributes['gen_ai.conversation.id'] as session_id, count(*) as commits,
    sum(attributes['loom.devtel.commit.insertions']::bigint) as insertions,
    sum(attributes['loom.devtel.commit.deletions']::bigint) as deletions
  from spool_spans where attributes['loom.devtel.git.event'] = 'post-commit' group by 1
)
select records.*, round((epoch_ms(end_ts) - epoch_ms(start_ts)) / 60000.0, 1) as wall_min,
  turns.* exclude (session_id), tools.* exclude (session_id), prs.prs, commits.* exclude (session_id)
from records left join turns using (session_id) left join tools using (session_id)
  left join prs using (session_id) left join commits using (session_id);

-- Checks run again with nothing changed in between. Two sources:
-- "same tree": a check that ran on the same working tree, with the same
--   result, as an earlier run of the same check: in the commit hook and
--   again before push, for example. Exact, from scripts/devtel/run.sh.
-- "no change": the same check command ran again in a session with no tool
--   call in between that may have changed the code (devtel_may_write).
--   An estimate from the transcripts, for history before the task wrapper.
-- "hook after check": a git commit or push whose hooks ran right after the
--   session ran a test or check, with no change in between. The hooks run
--   only some checks, so this is time spent re-checking, not all waste.
create or replace view devtel_reruns as
with step_runs as (
  select attributes['loom.devtel.step'] as step, attributes['loom.devtel.tree'] as tree,
    nullif(attributes['gen_ai.conversation.id'], '') as session_id,
    to_timestamp(start_ns::double / 1e9) as start_ts, (end_ns - start_ns) / 1e6 as ms,
    attributes['process.exit.code'] as exit_code
  from spool_spans where service = 'checks' and coalesce(attributes['loom.devtel.tree'], '') <> ''
),
same_tree as (
  select 'same tree' as kind, session_id, step as command, start_ts, ms,
    row_number() over (partition by step, tree, exit_code order by start_ts) as run
  from step_runs
),
ordered as (
  -- A commit or push does not change the working tree, though its message
  -- may be written with a heredoc.
  select *, sum(case when devtel_may_write(tool, command) and category not in ('git commit', 'git push') then 1 else 0 end)
      over (partition by session_id order by start_ts rows unbounded preceding) as changes
  from devtel_tools
),
no_edit as (
  select 'no change' as kind, session_id, normalized as command, start_ts,
    epoch_ms(end_ts) - epoch_ms(start_ts) as ms,
    row_number() over (partition by session_id, changes, normalized order by start_ts) as run
  from ordered
  where category in ('test', 'check', 'build') and not interrupted and not devtel_may_write(tool, command)
),
hook_after_check as (
  select 'hook after check' as kind, o.session_id, regexp_replace(regexp_replace(o.command, '^\s*cd\s+[^&;]+(&&|;)\s*', ''), '\s+', ' ', 'g') as command, o.start_ts,
    epoch_ms(o.end_ts) - epoch_ms(o.start_ts) as ms, 2 as run
  from ordered o
  where o.category in ('git commit', 'git push')
    -- A call that also runs a check itself would count that check as hook time.
    and not regexp_matches(o.command, '\b(cargo\s+(test|nextest|llvm-cov|clippy|build)|mise\s+run\s+(check|test|coverage|clippy)|prek\s+run)\b')
    and exists (select 1 from ordered c where c.session_id = o.session_id and c.changes = o.changes
      and c.category in ('test', 'check') and c.start_ts < o.start_ts)
)
select * from same_tree where run > 1
union all
select * from no_edit where run > 1
union all
select * from hook_after_check;

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
