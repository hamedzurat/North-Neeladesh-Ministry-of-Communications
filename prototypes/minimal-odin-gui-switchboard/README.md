# Minimal Odin GUI switchboard prototype

Throwaway Odin + raylib prototype for “Prototype the minimal Odin GUI
switchboard.” It implements the agreed standalone interaction model using
in-memory mock backend output; Cabinet Link integration comes later.

Run from the repository root:

```sh
just prototype-gui
```

The command expects the pinned Odin monthly release `dev-2026-08` on `PATH`.

Controls:

- Drag from any free female port to another free port to place one of eight
  Cords. Right-click either endpoint to disconnect it. A port can hold only one
  Cord.
- Hold one Operator/Police/EMS/Fire PTT or Tap Bridge listen button at a time.
- Scroll over Crank to fill its movement gauge; a full gauge flashes and resets.
- Drag the two tuning sliders.
- Change Directory digits with `+`/`-`; the mock e-paper briefly shows
  `SEARCHING` before returning a record.
- Click the speaker visualization to toggle mock playback.
- Let the sample receipt feed, then use the mouse wheel or scrollbar to browse
  it.

The Line Lamps, workday clock, e-paper pages, speaker action, and printer job are
mock backend outputs. They are not authoritative frontend state.
