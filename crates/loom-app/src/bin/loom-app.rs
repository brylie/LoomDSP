//! The `loom-app` program. Everything it does is in the `loom_app` library.

fn main() {
    if let Err(error) = loom_app::run() {
        eprintln!("loom-app: {error}");
        std::process::exit(1);
    }
}
