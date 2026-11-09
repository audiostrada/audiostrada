# Automation

Automation moves a control over time: a volume ride, a filter sweep, a send that opens on the
chorus.

## Lanes

Show a track's lanes with the **A** button of its header and add a lane with **+**: volume, pan,
mute, sends, and every parameter of its effects. A VCA has a volume lane, which moves every
track it controls.

- Click a lane to add a point, drag points to move them, double-click a point to delete it.
- Right-click a point for the shape of the curve to the next one: linear, smooth or step.
- Drag the lower edge of a lane to make it taller.

View › *Master track in the timeline* shows the master at the top, under the ruler. Its volume
is its main row, twice a lane's height (a fade at the end of the song); its **+** adds a lane for
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

