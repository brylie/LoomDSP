use super::*;

#[test]
fn a_line_is_key_value_pairs() {
    let w = Window {
        frames: 120,
        elapsed: Duration::from_secs(1),
        totals: RenderCounts {
            frames: 600,
            scene_builds: 1,
            control_builds: 2,
            scene_uploads: 1,
            control_uploads: 2,
            overlay_uploads: 0,
            trace_builds: 3,
            trace_uploads: 3,
        },
        worst: Duration::from_millis(9),
        causes: FrameCauses([2, 70, 1, 0, 3, 0, 44, 0]),
        cpu: 4.1,
    };
    assert_eq!(
        line(&w),
        "fps=120.0 avg_ms=8.33 worst_ms=9.00 frames=600 scene_builds=1 control_builds=2 \
             scene_uploads=1 control_uploads=2 overlay_uploads=0 trace_uploads=3 \
             causes=tick:2,live:70,meters:1,control:0,input:3,redraw:0,forced:44,other:0 cpu=4.1"
    );
}

#[test]
fn an_idle_line_has_no_frames() {
    let w = Window {
        frames: 0,
        elapsed: Duration::from_secs(1),
        totals: RenderCounts::default(),
        worst: Duration::ZERO,
        causes: FrameCauses::default(),
        cpu: 0.0,
    };
    assert!(line(&w).starts_with("fps=0.0 avg_ms=0.00 worst_ms=0.00 frames=0 "));
}

#[test]
fn bench_seconds_must_be_a_usable_duration() {
    assert_eq!(seconds("10"), Ok(10.0));
    assert_eq!(seconds("0.5"), Ok(0.5));
    for bad in ["inf", "NaN", "-1", "1e30", "ten"] {
        assert!(seconds(bad).is_err(), "{bad}");
    }
}

#[test]
fn report_only_with_a_flag() {
    assert!(Options::default().reporter().is_none());
    let idle = Options {
        measure_idle: true,
        ..Default::default()
    };
    assert!(idle.reporter().is_some());
    assert!(!idle.redraws());
}

/// A measured run reports a settled interval and then ends on time.
#[test]
fn measured_run_reports_and_finishes() {
    use iced::futures::StreamExt;
    let reporter = Reporter {
        seconds: Some(Duration::from_millis(2_100)),
    };
    let started = Instant::now();
    let mut updates = stream(&reporter);
    assert_eq!(iced::futures::executor::block_on(updates.next()), Some(()));
    assert!(started.elapsed() >= Duration::from_secs(2));
}

/// A zero wall interval has no CPU percentage; later CPU time is a
/// percentage of one core.
#[test]
fn cpu_percent_uses_the_report_interval() {
    assert_eq!(
        cpu_percent(Duration::ZERO, Duration::from_millis(1), Duration::ZERO),
        0.0
    );
    assert_eq!(
        cpu_percent(
            Duration::from_millis(10),
            Duration::from_millis(60),
            Duration::from_millis(100)
        ),
        50.0
    );
}
