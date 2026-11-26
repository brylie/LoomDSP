-- The devtel archive (docs/dev/devtel.md). Tables only grow: every row has a
-- natural key, and the import inserts with "or ignore", so importing twice
-- changes nothing and rows outlive the files they came from.
--
-- The archive outlives the transcripts it came from, so a column dropped
-- here is lost for good. Add columns with `alter table ... add column if not
-- exists` below the table, never by changing a create statement, so an
-- existing archive gains them too.

create table if not exists devtel_meta (key varchar primary key, value varchar);
insert or ignore into devtel_meta values ('schema_version', '1');

-- One row per file the import has read, at the size it had then. A file
-- whose size or time changed is read again; rows it gave before are kept.
create table if not exists imported_files (
  kind varchar,
  path varchar primary key,
  size bigint,
  mtime bigint,
  imported_at timestamptz
);

-- One row per Claude Code transcript record, reduced to metadata and
-- command lines (ADR-0048): no prompt text, replies, or tool output. A
-- record without a uuid (PR links, cost totals, titles) is keyed on a hash
-- of its JSON. Claude Code writes one assistant record per content block,
-- each repeating the message's usage, so count tokens per message_id
-- (devtel_turns), never per row.
create table if not exists claude_records (
  session_id varchar,
  record_key varchar,
  uuid varchar,
  parent_uuid varchar,
  agent_id varchar,
  type varchar,
  subtype varchar,
  ts timestamptz,
  cwd varchar,
  git_branch varchar,
  version varchar,
  entrypoint varchar,
  is_sidechain boolean,
  -- Assistant turns.
  message_id varchar,
  request_id varchar,
  model varchar,
  requested_model varchar,
  advisor_model varchar,
  effort varchar,
  thinking_ms bigint,
  stop_reason varchar,
  api_error varchar,
  input_tokens bigint,
  output_tokens bigint,
  cache_read_tokens bigint,
  cache_write_tokens bigint,
  -- A person typed this message (not a tool result or a system message).
  is_prompt boolean,
  prompt_chars integer,
  tool_calls struct(
    id varchar, name varchar, command varchar, description varchar,
    file_path varchar, pattern varchar, subagent_type varchar, model varchar,
    run_in_background boolean, skill varchar, url varchar
  )[],
  tool_results struct(id varchar, is_error boolean)[],
  -- From the tool result of a user record: Bash, Agent, and git details.
  result_interrupted boolean,
  result_git_operation varchar,
  result_agent_id varchar,
  result_model varchar,
  -- Session-level records.
  pr_number integer,
  pr_repository varchar,
  title varchar,
  cost_usd double,
  api_ms bigint,
  tool_ms bigint,
  lines_added bigint,
  lines_removed bigint,
  model_usage json,
  source_file varchar,
  primary key (session_id, record_key)
);

-- One row per subagent, from its .meta.json file next to its transcript.
create table if not exists claude_agents (
  session_id varchar,
  agent_id varchar,
  agent_type varchar,
  description varchar,
  tool_use_id varchar,
  model varchar,
  spawn_depth integer,
  source_file varchar,
  primary key (session_id, agent_id)
);

-- One row per span from the spool (mise tasks, git events, hooks).
create table if not exists spool_spans (
  trace_id varchar,
  span_id varchar,
  parent_span_id varchar,
  name varchar,
  service varchar,
  start_ns hugeint,
  end_ns hugeint,
  error boolean,
  attributes map(varchar, varchar),
  resource map(varchar, varchar),
  primary key (trace_id, span_id)
);
