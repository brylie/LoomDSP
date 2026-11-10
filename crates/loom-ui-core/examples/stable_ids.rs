//! Keeps a number on each row of a list while rows come and go.
//!
//! This uses only `loom-ui-core`. A list of files is shown three times. A
//! row keyed by its file name keeps its number when the row above it is
//! removed, and a row that comes back gets a new number. Run it with:
//!
//! ```sh
//! cargo run -p loom-ui-core --example stable_ids
//! ```

use loom_ui_core::{IdTable, UiId};

/// Shows one build of the list: asks the table for each row's number, then
/// lets the table drop the rows that are no longer shown.
fn build(table: &mut IdTable, files: &[&str]) {
    let list = UiId::panel("files");
    for file in files {
        let row = list.clone().with(*file);
        println!("  {file:<10} is element {}", table.number(&row));
    }
    table.retain_seen();
}

/// Builds the list three times with different rows.
fn main() {
    let mut table = IdTable::new();

    println!("three files:");
    build(&mut table, &["a.txt", "b.txt", "c.txt"]);

    // "c.txt" moves up a row and keeps its number, because its key is its
    // name and not its position.
    println!("b.txt removed:");
    build(&mut table, &["a.txt", "c.txt"]);

    // A new "b.txt" is a new element, so it gets a number nothing had.
    println!("b.txt added again:");
    build(&mut table, &["a.txt", "b.txt", "c.txt"]);
}
