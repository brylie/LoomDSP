-- The devtel archive (docs/dev/devtel.md). Tables only grow: every row has a
-- natural key, and the import inserts with "or ignore", so importing twice
-- changes nothing and rows outlive the files they came from.

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
-- command lines (ADR-0048): no prompt text, replies, or tool output.
create table if not exists claude_records (
  session_id varchar,
  uuid varchar,
  parent_uuid varchar,
  agent_id varchar,
  type varchar,
  subtype varchar,
  ts timestamptz,
  cwd varchar,
  git_branch varchar,
  version varchar,
  is_sidechain boolean,
  model varchar,
  stop_reason varchar,
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
  tool_results struct(id varchar, is_error boolean, interrupted boolean)[],
  source_file varchar,
  primary key (session_id, uuid)
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
