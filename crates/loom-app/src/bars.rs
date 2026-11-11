//! The stats bar and the status line as plain data: each item's text, its
//! hover text, and how loudly it is shown. The window draws the bar from
//! this model and the accessibility tree reads the same one, so they cannot
//! disagree (`docs/design/ui-layer.md`, section 7, rule 1).

use crate::{App, alerts};
use std::sync::atomic::Ordering;
use std::time::Instant;

/// What kind of thing a bar item is, for a screen reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Plain text that changes with the patch or the devices.
    Text,
    /// A reading that moves while audio plays: the load, the dropouts. A
    /// live value (ui-layer.md, section 3, rule 7).
    Live,
    /// A warning meant to be noticed: it is announced.
    Alert,
}

/// One item of the stats bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    /// Its fixed name, the last key of its UI ID: `load` in `stats/load`.
    pub name: &'static str,
    /// Its UI ID as text, for the container that wraps it in the window.
    pub id: &'static str,
    /// The text shown.
    pub text: String,
    /// Its hover text, if it has one.
    pub hover: Option<String>,
    /// How loudly it is shown. `None` is the window's plain text colour.
    pub tone: Option<alerts::Severity>,
    /// What it is.
    pub kind: Kind,
}

/// One message at the end of the bar: the status, or a kept error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    /// The text shown, cut off by the window if it is long.
    pub shown: String,
    /// All of it, in hover text.
    pub hover: String,
    /// How loudly it is shown.
    pub severity: alerts::Severity,
    /// The problem its × dismisses, if it has a ×.
    pub dismiss: Option<alerts::Cause>,
}

impl Field {
    /// The hover text of the × that dismisses it.
    pub(crate) fn dismiss_hover(&self) -> &'static str {
        if self.severity == alerts::Severity::Warning {
            "Dismiss this warning"
        } else {
            "Dismiss this error"
        }
    }
}

/// The developer switches at the start of the bar, shown with `--dev` or a
/// measuring flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DevTools {
    /// Whether **Measure** is on.
    pub measuring: bool,
    /// Whether **Move camera** is on.
    pub animating: bool,
    /// The frame rate text. It changes every frame while measuring, so a
    /// screen reader is not given it.
    pub frames: String,
}

/// The whole bar, left to right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Model {
    /// The developer switches, when they show.
    pub dev: Option<DevTools>,
    /// The fixed items.
    pub items: Vec<Item>,
    /// The kept error with a field of its own, if one shows.
    pub kept: Option<Field>,
    /// The status line.
    pub status: Field,
}

/// The UI ID text of the status line, the kept error, and their × buttons.
pub(crate) const STATUS_ID: &str = "status";
/// See [`STATUS_ID`].
pub(crate) const STATUS_DISMISS_ID: &str = "status/dismiss";
/// See [`STATUS_ID`].
pub(crate) const KEPT_ID: &str = "status/kept";
/// See [`STATUS_ID`].
pub(crate) const KEPT_DISMISS_ID: &str = "status/kept/dismiss";
/// The UI ID text of the **Measure** switch.
pub(crate) const MEASURE_ID: &str = "stats/measure";
/// The UI ID text of the **Move camera** switch.
pub(crate) const MOVE_CAMERA_ID: &str = "stats/move_camera";

impl App {
    /// The stats bar and the status line as they are now.
    pub(crate) fn bars_model(&self) -> Model {
        use alerts::Severity;
        let graph = self.session.controller.graph();
        let item = |name, id, text: String, tone, kind| Item {
            name,
            id,
            text,
            hover: None,
            tone,
            kind,
        };
        let mut items = vec![item(
            "patch",
            "stats/patch",
            format!("{} modules, {} cables", graph.len(), graph.cables().count()),
            None,
            Kind::Text,
        )];
        // Load and dropouts are always shown, and turn amber or red before
        // a problem is audible (FR-PERF-04).
        // The snapshot reads the same model, so the two cannot disagree.
        let stats = self.stats_model();
        items.push(item(
            "audio",
            "stats/audio",
            stats.label.clone(),
            Some(Severity::Normal),
            Kind::Text,
        ));
        if stats.playing {
            items.push(item(
                "load",
                "stats/load",
                stats.load_text(),
                Some(alerts::load_severity(self.session.engine.stream.load().0)),
                Kind::Live,
            ));
            items.push(item(
                "dropouts",
                "stats/dropouts",
                format!("dropouts {}", stats.dropouts),
                Some(self.alerts.dropout_severity(Instant::now())),
                Kind::Live,
            ));
            if let Some((underruns, overruns)) = stats.input_alert {
                items.push(item(
                    "input",
                    "stats/input",
                    format!("input underruns {underruns}, overruns {overruns}"),
                    Some(Severity::Warning),
                    Kind::Alert,
                ));
            }
            let engine = self.session.controller.stats();
            let since_start = |count: &std::sync::atomic::AtomicU64| {
                format!("{} samples since the engine started", count.load(Ordering::Relaxed))
            };
            if stats.output_silenced {
                items.push(Item {
                    hover: Some(since_start(&engine.output_silenced)),
                    ..item(
                        "silenced",
                        "stats/silenced",
                        "Output: bad samples silenced".to_string(),
                        Some(Severity::Alarm),
                        Kind::Alert,
                    )
                });
            }
            if stats.output_clipped {
                items.push(Item {
                    hover: Some(since_start(&engine.output_clipped)),
                    ..item(
                        "clipped",
                        "stats/clipped",
                        "Output clipped".to_string(),
                        Some(Severity::Warning),
                        Kind::Alert,
                    )
                });
            }
            if stats.telemetry_dropped > 0 {
                items.push(item(
                    "telemetry",
                    "stats/telemetry",
                    format!("telemetry dropped {}", stats.telemetry_dropped),
                    Some(Severity::Normal),
                    Kind::Text,
                ));
            }
        }
        // A count, with the names in hover text, so a long list never
        // pushes the other items off the window (visual-system.md, rule 7).
        let ports = self.rig.ports.as_slice();
        items.push(Item {
            hover: (!ports.is_empty()).then(|| ports.iter().map(|port| port.name()).collect::<Vec<_>>().join("\n")),
            ..item(
                "midi",
                "stats/midi",
                match ports.len() {
                    0 => "MIDI: no inputs".to_string(),
                    1 => "MIDI: 1 input".to_string(),
                    n => format!("MIDI: {n} inputs"),
                },
                Some(Severity::Normal),
                Kind::Text,
            )
        });
        // A word, so it reads without colour, and it stays whatever the
        // status says: in learn mode screen controls do not change values.
        if self.session.learn.is_on() {
            items.push(item(
                "learning",
                "stats/learning",
                "Learning".to_string(),
                None,
                Kind::Text,
            ));
        }
        // The kept error and the status share the remaining width.
        let kept = self.alerts.kept_field().map(|kept| Field {
            shown: alerts::labelled(kept.severity, &kept.text),
            hover: self.alerts.kept_hover(),
            severity: kept.severity,
            dismiss: kept.dismissible.then_some(kept.cause),
        });
        let shown = alerts::labelled(self.alerts.severity(), self.alerts.status());
        let status = Field {
            hover: if self.alerts.status_is_kept() {
                self.alerts.kept_hover()
            } else {
                shown.clone()
            },
            shown,
            severity: self.alerts.severity(),
            dismiss: self
                .alerts
                .top_kept()
                .filter(|kept| kept.dismissible && self.alerts.kept_field().is_none())
                .map(|kept| kept.cause),
        };
        Model {
            dev: self.dev_tools.then(|| DevTools {
                measuring: self.measuring,
                animating: self.animating,
                frames: if self.measuring {
                    self.meter.stats().summary()
                } else {
                    "redraws on change".to_string()
                },
            }),
            items,
            kept,
            status,
        }
    }
}

#[cfg(test)]
mod tests;
