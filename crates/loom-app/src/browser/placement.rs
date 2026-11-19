//! Toolbar placement uses the finished tile's size, including exposed
//! controls on sub-patches and the ports of empty slots.

use crate::App;
use loom_canvas::{BrowserPlacement, GRID};
use loom_core::ModuleId;
use loom_ui_core::{Point, Rect, Size};
use std::collections::BTreeSet;

impl App {
    /// Selects and places the new tile within the add's undo step. The
    /// commit builds its actual bounds before toolbar placement uses them.
    pub(crate) fn commit_added_tile(&mut self, id: ModuleId, placement: BrowserPlacement) -> bool {
        let start = match placement {
            BrowserPlacement::Pointer(point) => point,
            BrowserPlacement::Toolbar(view) => super::snap(view.center()),
        };
        self.session.positions.insert(id.0, (start.x, start.y));
        self.session.selection = BTreeSet::from([id.0]);
        let sent = self.commit();
        if let BrowserPlacement::Toolbar(view) = placement
            && let Some(tile) = self.session.scene.module(id.0)
        {
            let size = tile.bounds().size();
            let occupied = self
                .session
                .scene
                .modules()
                .iter()
                .filter(|other| other.id != id.0)
                .map(|other| other.bounds())
                .collect();
            let point = nearest_free(view, size, occupied);
            if point != start {
                self.session.positions.insert(id.0, (point.x, point.y));
                self.session.scene.move_modules(&[id.0], point - start);
            }
        }
        sent
    }
}

/// Finds the nearest free grid corner, preferring positions that show the
/// whole tile. Between two obstacle edges the same columns are free, so
/// only rows at an edge or nearest the centre need to be searched.
fn nearest_free(view: Rect, size: Size, mut occupied: Vec<Rect>) -> Point {
    let down = |v: f32| (v / GRID).floor() * GRID;
    let up = |v: f32| (v / GRID).ceil() * GRID;
    let centre = view.center();
    // At half a step prefer the earlier row, then the left column.
    let nearest = |v: f32| (v / GRID - 0.5).ceil() * GRID;
    let start = Point::new(nearest(centre.x), nearest(centre.y));
    occupied.sort_by(|a, b| a.x.total_cmp(&b.x));
    let search = |left: f32, right: f32, top: f32, bottom: f32| {
        if left > right || top > bottom {
            return None;
        }
        let mut rows = vec![start.y.clamp(top, bottom)];
        for r in &occupied {
            rows.extend([down(r.y - size.height - GRID), up(r.y + r.height + GRID)]);
        }
        rows.sort_by(f32::total_cmp);
        rows.dedup();
        let mut best = None;
        let mut distance = f64::INFINITY;
        for y in rows.into_iter().filter(|y| (top..=bottom).contains(y)) {
            let mut consider = |from: f32, to: f32| {
                if from <= to {
                    let x = start.x.clamp(from, to);
                    let d = (f64::from(x) - f64::from(centre.x)).powi(2) + (f64::from(y) - f64::from(centre.y)).powi(2);
                    if d < distance {
                        distance = d;
                        best = Some(Point::new(x, y));
                    }
                }
            };
            let mut from = left;
            for r in &occupied {
                if y + size.height + GRID <= r.y || y >= r.y + r.height + GRID {
                    continue;
                }
                consider(from, right.min(down(r.x - size.width - GRID)));
                from = from.max(up(r.x + r.width + GRID));
                if from > right {
                    break;
                }
            }
            consider(from, right);
        }
        best
    };
    search(
        up(view.x),
        down(view.x + view.width - size.width),
        up(view.y),
        down(view.y + view.height - size.height),
    )
    .or_else(|| search(f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY))
    .unwrap_or(start)
}

#[cfg(test)]
mod tests;
