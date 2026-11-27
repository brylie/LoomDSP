-- The devtel efficiency report for the last $DAYS days (mise run devtel-report).
create or replace temp view recent as
  select * from devtel_tools where start_ts >= now() - to_days($DAYS);

.print '== Sessions and agents'
select count(distinct session_id) as sessions,
  count(distinct agent_id) as subagents,
  count(*) as tool_calls,
  round(sum(epoch_ms(end_ts) - epoch_ms(start_ts)) / 3.6e6, 1) as tool_hours
from recent;

.print '== Where the time goes (tool time by kind of work)'
select category, count(*) as calls, round(sum(epoch_ms(end_ts) - epoch_ms(start_ts)) / 60000.0) as minutes,
  round(median(epoch_ms(end_ts) - epoch_ms(start_ts)) / 1000.0, 1) as median_s,
  round(100.0 * count(*) filter (where error) / count(*), 1) as error_pct
from recent group by 1 order by minutes desc;

.print '== Checks run again with nothing changed (see devtel_reruns)'
select kind, count(*) as reruns, round(sum(ms) / 60000.0) as minutes
from devtel_reruns where start_ts >= now() - to_days($DAYS) group by 1 order by 1;

select kind, left(regexp_replace(command, '\s+', ' ', 'g'), 70) as command, count(*) as reruns, round(sum(ms) / 60000.0) as minutes
from devtel_reruns where start_ts >= now() - to_days($DAYS)
group by all order by minutes desc limit 10;

.print '== Checks that reused a pass on the same tree (docs/dev/check-reuse.md)'
select attributes['loom.devtel.step'] as check_name, count(*) as reused,
  round(sum(attributes['loom.devtel.reused_ms']::bigint) / 60000.0, 1) as minutes_saved
from spool_spans
where service = 'checks' and attributes['loom.devtel.reused'] = 'true'
  and to_timestamp(start_ns::double / 1e9) >= now() - to_days($DAYS)
group by 1 order by minutes_saved desc;

.print '== Possible loops: one command 3+ times within 10 calls (see devtel_loops)'
select left(command, 70) as command, count(*) as sessions, max(times_in_ten_calls) as worst_burst,
  sum(times_in_session) as runs, round(sum(minutes)) as minutes
from devtel_loops where first_ts >= now() - to_days($DAYS)
group by 1 order by minutes desc limit 10;

.print '== Test runs and the test queue (docs/dev/test-runs.md)'
select coalesce(attributes['loom.test.selection'], 'queued command') as selection, count(*) as runs,
  round(sum(end_ns - start_ns) / 6e10) as run_minutes,
  round(median(end_ns - start_ns) / 1e9) as median_s
from spool_spans
where service = 'tests' and name = 'test run' and to_timestamp(start_ns::double / 1e9) >= now() - to_days($DAYS)
group by 1 order by runs desc;

-- A cancelled wait has no loom.test.wait_ms; its span's length is the wait.
select count(*) as waits, count(*) filter (where wait_ms > 2000) as waited_over_2s,
  round(sum(wait_ms) / 60000.0, 1) as wait_minutes,
  round(max(wait_ms) / 60000.0, 1) as longest_wait_min,
  count(*) filter (where cancelled) as cancelled
from (
  select coalesce(attributes['loom.test.wait_ms']::bigint, (end_ns - start_ns) // 1000000) as wait_ms,
    coalesce(attributes['loom.test.cancelled'] = 'true', false) as cancelled
  from spool_spans
  where service = 'tests' and name = 'test queue wait' and to_timestamp(start_ns::double / 1e9) >= now() - to_days($DAYS)
);

.print '== Models (one row per API response; tokens counted once per message)'
select model, count(*) as turns, count(distinct session_id) as sessions,
  round(sum(output_tokens) / 1e6, 2) as output_mtok,
  round(sum(input_tokens + coalesce(cache_read_tokens, 0) + coalesce(cache_write_tokens, 0)) / 1e6, 1) as input_mtok,
  round(100.0 * sum(cache_read_tokens) / nullif(sum(input_tokens + cache_read_tokens + coalesce(cache_write_tokens, 0)), 0), 1) as cache_hit_pct,
  string_agg(distinct effort, ', ') as efforts
from devtel_turns where ts >= now() - to_days($DAYS)
group by 1 order by turns desc;

.print '== Cost and model by subagent type (token shares; the archive has cost per session only)'
-- A model change for an agent type shows as a new row with its own first
-- and last day, so a later report can compare the two.
with turns as (
  select tu.*, coalesce(a.agent_type, case when tu.agent_id is null then '(main thread)' else '?' end) as agent_type
  from devtel_turns tu left join claude_agents a using (session_id, agent_id)
  where tu.ts >= now() - to_days($DAYS)
),
sized as (
  select *, input_tokens + coalesce(cache_read_tokens, 0) + coalesce(cache_write_tokens, 0) as all_input from turns
)
-- A subagent's ID is unique only within its session, so count the pair.
select agent_type, model, count(distinct session_id || ':' || coalesce(agent_id, '')) as agents, count(*) as turns,
  round(sum(output_tokens) / 1e6, 2) as output_mtok, round(sum(all_input) / 1e6, 1) as input_mtok,
  round(100.0 * sum(all_input) / sum(sum(all_input)) over (), 1) as input_share_pct,
  round(sum(all_input) / count(*) / 1000) as input_k_per_turn,
  min(ts)::date as first_day, max(ts)::date as last_day
-- No row limit: a small new row is the one a model change adds.
from sized group by all order by input_mtok desc;

.print '== Work done (from the git hooks; empty until they are installed)'
select count(*) as commits,
  count(*) filter (where attributes['loom.devtel.commit.agent_coauthored'] = 'true') as agent_commits,
  sum(attributes['loom.devtel.commit.insertions']::bigint) as insertions,
  sum(attributes['loom.devtel.commit.deletions']::bigint) as deletions,
  count(distinct attributes['vcs.ref.head.name']) as branches
from spool_spans
where attributes['loom.devtel.git.event'] = 'post-commit' and to_timestamp(start_ns::double / 1e9) >= now() - to_days($DAYS);

.print '== Largest sessions (cost and lines from Claude Code; commits from the git hooks)'
select left(session_id, 8) as session, left(coalesce(title, git_branch), 40) as title,
  len(string_split(prs, ' ')) as prs, tool_calls, subagents,
  checking_min, waiting_min, round(cost_usd, 2) as cost_usd, lines_added, lines_removed, commits
from devtel_sessions where start_ts >= now() - to_days($DAYS)
order by tool_calls desc limit 10;
