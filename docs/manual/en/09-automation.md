# Automation

Automation moves a control over time: a volume ride, a filter sweep, a send that opens on the
chorus.

## Lanes

Show a track's lanes with the **A** button of its header and add a lane with **+**: volume, pan,
mute, sends, and every parameter of its effects. A VCA has a volume lane, which moves every
track it controls.

- Click a lane to add a point; click on the curve to add one exactly on it. Press and drag on
  an empty spot to place a point and move it at once.
- Drag a point to move it. It moves from where you grabbed it, without jumping under the
  pointer. Hold **Shift** to move it ten times more finely (the grid and the pulls are off), and
  **Ctrl** to change its value only, its time staying put.
- Drag the curve between two points to move that segment up or down with both of its points.
  Before the first point and after the last, the flat line moves that point.
- **Alt**+drag a segment to bend it: up, the curve bows upward; back to the middle, it becomes
  straight again.
- **Ctrl**+drag across a lane to draw freehand: the stroke replaces the points it passes over.
- A moved value is pulled to the dashed line of the set value, to 0 dB (or the center for
  the pan), and to the values of the neighbouring points, so flat lines stay flat; a dashed
  line in the lane's colour shows the pull. Times snap to the grid, the clips and the markers as
  clips do.
- Click a point to select it, **Shift**+click to add to the selection, **Shift**+drag (or drag
  with the range tool) to select points in a rectangle. Selected points move together, and
  **Delete** removes them. **Alt**+click deletes a point.
- Double-click a point to type its value: "-3.5", "-3.5 dB", "L 20", "2.4 kHz".
- Right-click a point for the shape of the curve to the next one (linear, smooth or step), for
  the selected points together. Right-click elsewhere in the lane to select or delete every point.
- Volume lanes follow the fader's scale, finer around 0 dB. Hovering a lane tells what a click
  or a drag would do there.
- Drag the lower edge of a lane to make it taller.

View › *Master track in the timeline* shows the master at the top, under the ruler. Its volume
is its main row, taller than a lane (a fade at the end of the song); its **+** adds a lane for
a parameter of its effects. Every row is edited the same way and resized by its lower edge. Its automation mode is on its strip in the mixer
(Read by default). Its **M** and **Mono** are those of its strip. Right-click its name to hide
it, the empty space under the track headers to show it again; the same option is in Preferences
› Appearance.

## Modes

The mode button beside the pan (**Read**, **Touch**...) decides what moving a control does:

| Mode | Behaviour |
|---|---|
| **Off** | the lanes are ignored |
| **Read** | the lanes play |
| **Touch** | writes while you move a control, returns to the lane when you let go |
| **Latch** | writes while you move a control, keeps the last value until playback stops |
| **Write** | records every control while playing |
| **Trim** | offsets the existing curve |

While you hold an automated control in Read, it follows your hand; it gives the control back to
the lane when you let go.

