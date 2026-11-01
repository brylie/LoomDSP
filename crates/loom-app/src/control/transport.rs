//! Where commands come from: a Unix socket or a script file, one JSON command per line, and the reply written back.

use super::*;

/// Where commands come from.
#[derive(Debug, Clone)]
pub enum Listener {
    Socket(PathBuf),
    Script(PathBuf),
}

impl Hash for Listener {
    // One listener per source for the whole run: opening another patch
    // keeps it going.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-app control".hash(state);
        match self {
            Self::Socket(path) => ("socket", path).hash(state),
            Self::Script(path) => ("script", path).hash(state),
        }
    }
}

/// Removes the control sockets, as the app exits. The listener threads
/// would remove them too, but the process may end first.
pub fn close(listeners: &[Listener]) {
    for listener in listeners {
        if let Listener::Socket(path) = listener {
            remove_socket(path);
        }
    }
}

/// Removes a socket file, and never any other kind of file: a mistaken
/// `--control` path must not delete a user's file.
pub(super) fn remove_socket(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = std::fs::remove_file(path);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// How long a listener waits for the app to answer one command.
pub(super) const ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs a listener on its own thread and yields each command for the app.
pub fn stream(listener: &Listener) -> impl Stream<Item = Call> + use<> {
    let (tx, rx) = mpsc::channel(1);
    let listener = listener.clone();
    std::thread::spawn(move || match listener {
        Listener::Script(path) => match std::fs::File::open(&path) {
            Ok(file) => {
                let mut tx = tx;
                // Only a script reads the environment (V1).
                let env = |name: &str| std::env::var(name).ok();
                serve(BufReader::new(file), std::io::stdout(), &mut tx, Some(&env));
            }
            Err(e) => eprintln!("loom-app: control script {}: {e}", path.display()),
        },
        Listener::Socket(path) => listen(&path, tx),
    });
    rx
}

#[cfg(unix)]
pub(super) fn listen(path: &std::path::Path, mut tx: mpsc::Sender<Call>) {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener;
    // A socket left by an earlier run is replaced; any other file is not.
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_socket() {
            remove_socket(path);
        } else {
            eprintln!("loom-app: control: {} exists and is not a socket", path.display());
            return;
        }
    }
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("loom-app: control socket {}: {e}", path.display());
            return;
        }
    };
    eprintln!("loom-app: control channel on {}", path.display());
    // Poll, so the thread ends when the app closes.
    let _ = listener.set_nonblocking(true);
    while !tx.is_closed() {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                if let Ok(reader) = stream.try_clone() {
                    serve(BufReader::new(reader), stream, &mut tx, None);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                eprintln!("loom-app: control socket: {e}");
                break;
            }
        }
    }
    remove_socket(path);
}

#[cfg(not(unix))]
pub(super) fn listen(_path: &std::path::Path, _tx: mpsc::Sender<Call>) {
    eprintln!("loom-app: --control needs Unix domain sockets, which this platform lacks; use --control-script");
}

/// Reads a variable for a script (`None` on the socket, rule V1).
pub(super) type Env<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

/// The exit status the app ends with, set when a script stops early
/// (Script variables, V4). 0 until then.
pub(super) static EXIT_STATUS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Exit status for a script that could not run, as for `loom analyze`.
pub(super) const COULD_NOT_RUN: i32 = 2;

/// The exit status a script asked for, or 0.
pub fn exit_status() -> i32 {
    EXIT_STATUS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Reads commands line by line, has the app run each one, and writes each
/// reply as a line. Returns when the input ends or the app closes. With
/// `env`, `${NAME}` in string values is replaced first, and an unset
/// variable ends the script and quits the app with status 2.
pub(super) fn serve(input: impl BufRead, mut output: impl Write, tx: &mut mpsc::Sender<Call>, env: Env) {
    for line in input.lines() {
        let Ok(line) = line else { return };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, answer, vars, stop) = run_line(line, tx, env);
        let mut reply = Map::new();
        if let Some(id) = id {
            reply.insert("id".to_string(), id);
        }
        match answer {
            Ok(fields) => {
                reply.insert("ok".to_string(), Value::Bool(true));
                reply.extend(fields);
            }
            Err(error) => {
                reply.insert("ok".to_string(), Value::Bool(false));
                reply.insert("error".to_string(), Value::String(error));
            }
        }
        if !vars.is_empty() {
            reply.insert("vars".to_string(), Value::Object(vars));
        }
        let write_failed = writeln!(output, "{}", Value::Object(reply)).is_err() || output.flush().is_err();
        // A script that must stop still quits with its status, even when
        // its reply could not be written (stdout closed).
        if stop {
            EXIT_STATUS.store(COULD_NOT_RUN, std::sync::atomic::Ordering::Relaxed);
            let _ = send(Command::Quit, tx);
            return;
        }
        if write_failed {
            return;
        }
        if tx.is_closed() {
            return;
        }
    }
}

/// Parses one line and gets its answer, with the variables it used, and
/// whether the script must stop because one was unset.
pub(super) fn run_line(
    line: &str,
    tx: &mut mpsc::Sender<Call>,
    env: Env,
) -> (Option<Value>, Answer, Map<String, Value>, bool) {
    let mut value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => return (None, Err(format!("not JSON: {e}")), Map::new(), false),
    };
    let id = value.as_object_mut().and_then(|v| v.remove("id"));
    let vars = match env.map(|env| crate::script_vars::substitute(&mut value, &env)) {
        Some(Err(unset)) => return (id, Err(format!("{unset}; the script stops here")), Map::new(), true),
        Some(Ok(vars)) => vars,
        None => Map::new(),
    };
    let command: Command = match serde_json::from_value(value) {
        Ok(command) => command,
        Err(e) => return (id, Err(e.to_string()), vars, false),
    };
    (id, send(command, tx), vars, false)
}

/// Has the app run one command and waits for its answer.
pub(super) fn send(command: Command, tx: &mut mpsc::Sender<Call>) -> Answer {
    if let Command::Wait { ms } = command {
        std::thread::sleep(Duration::from_millis(ms));
        return Ok(Map::new());
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command,
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    if iced::futures::executor::block_on(tx.send(call)).is_err() {
        return Err("the app closed".to_string());
    }
    receiver
        .recv_timeout(ANSWER_TIMEOUT)
        .unwrap_or_else(|_| Err("the app did not answer".to_string()))
}
