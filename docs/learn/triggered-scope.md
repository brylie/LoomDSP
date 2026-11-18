# Why a triggered scope holds still

A scope draws a signal against time. A trigger lets it put the start of each trace at the same place in the wave. For a rising trigger at zero, the scope looks for a sample below zero followed by one at or above zero. If the samples read −0.2 and +0.6, the straight line between them crosses zero one quarter of the way across. LoomDSP shifts the trace left by that fraction of a sample, putting the crossing at the left edge.

```text
              Trigger                      Right edge
                 |                             |
First read     ●-|------●--------●--------●----|---●
Later read   ●---|----●--------●--------●------|-●
                 |                             |
```

The dots show sample times. In each trace the left edge sits at the zero crossing. If a wave takes 183.47 samples to repeat, the next crossing falls 0.47 samples further along the sampling grid. After the shift, the dots move along the wave between traces, although the wave stays in the same place. Comparing dot 1 with dot 1 would say it moved. The scope instead measures how far each dot lies from the line through the other trace. It asks for a redraw when the line has moved more than half a drawn pixel. A sharp corner can still visibly move as the sample positions change. [Trace comparison in the app](../../crates/loom-app/src/monitor.rs)

VCV Scope uses a rising voltage crossing to align repeating waves too. In LoomDSP, the trigger chooses where the trace starts, and the half-pixel rule decides whether the window needs a new frame. [VCV Scope documentation](https://vcvrack.com/Free#vcv-scope)

A trace begins with the sample just before the crossing. At the far end it also needs one sample beyond the right edge so a straight segment reaches the edge. A four-interval window needs five samples when it starts exactly on a sample. Shift those five left by a quarter of an interval and the last point lands at 3.75. A sixth sample lands at 4.75 and gives the line a segment that the frame can clip at 4. When LoomDSP reduces a long trace to fewer points, it keeps the first and last available samples so the line still reaches the edges. [Panels specification](../design/panels.md#drawing-the-scope-trace)
