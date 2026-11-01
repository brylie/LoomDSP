use super::*;

#[test]
fn a_cell_answers_only_its_owner() {
    let table = LiveTable::new(2);
    assert_eq!(table.read(0, 7), None);
    table.claim(0, 7);
    table.write(0, 0.25);
    assert_eq!(table.read(0, 7), Some(0.25));
    assert_eq!(table.read(0, 8), None);
    // Moved to another parameter: the old one reads nothing, and the
    // new one has no value until a block stores one.
    table.claim(0, 8);
    assert_eq!(table.read(0, 7), None);
    assert_eq!(table.read(0, 8), None);
    table.write(0, 0.0);
    assert_eq!(table.read(0, 8), Some(0.0));
    table.release(0);
    assert_eq!(table.read(0, 8), None);
}

#[test]
fn claiming_the_same_owner_keeps_the_value() {
    let table = LiveTable::new(1);
    table.claim(0, 7);
    table.write(0, 0.5);
    table.claim(0, 7);
    assert_eq!(table.read(0, 7), Some(0.5));
}

#[test]
fn cells_stay_with_their_parameter_and_run_out() {
    let mut cells = LiveCells::new(2);
    assert_eq!(cells.assign(&[10, 20]), (vec![], 0));
    let ten = cells.cell(10).unwrap();
    // A third parameter finds the table full.
    assert_eq!(cells.assign(&[10, 20, 30]), (vec![], 1));
    assert_eq!(cells.cell(30), None);
    // 20 goes away; its cell comes back and 30 gets it. 10 keeps its own.
    let twenty = cells.cell(20).unwrap();
    assert_eq!(cells.assign(&[10, 30]), (vec![twenty], 0));
    assert_eq!(cells.cell(10), Some(ten));
    assert_eq!(cells.cell(30), Some(twenty));
}
