# Recording

## Record a take

1. Choose the track's input in its header: *In 1*, or *In 1-2* for a stereo source. Track › *Add
   stereo track* ([[Ctrl+Alt+T]]) makes a track that records two inputs.
2. Arm the track with its **R** button: its input is heard through the track and its meter shows
   the input level.
3. Press record ([[R]]). Several armed tracks record together.
4. Press [[Space]] to stop. The take is a new clip; its file goes into the project's `Audio`
   folder.

## Monitoring

The speaker button beside **R** says when a track hears its input:

| Setting | The track hears its input |
|---|---|
| **Auto** | while armed, as set in Preferences (always, or only while stopped or recording) |
| **Input** | always, armed or not: to play through an amp simulator |
| **Off** | never: for direct monitoring on the audio interface |

The button is lit while the input is heard. An armed track's meter shows its input level,
heard or not.

## Low latency mode

The stopwatch in the transport bar bypasses the effects that add more than 1 ms, except on the
tracks being played, so that their latency no longer delays what you hear. The mixer shows the
bypassed effects struck through. Turn it off to mix.

## Metronome

The metronome button ([[K]]) clicks on every beat while playing. Right-click it, or
[[Shift+K]], for its window: volume, sound, clicks per beat, accents, groups for odd meters (7/8
as 2+2+3), clicking only while recording, and a count-in of up to four bars. The click is never
exported.

## Punch and loop recording

- **Punch** ([[P]]): Shift-drag in the ruler sets the punch range, or right-click the ruler.
  Recording only happens inside it, so that a mistake can be replaced without touching the rest.
- **Loop recording**: with the loop on ([[L]]), each pass becomes a take. Takes are kept in
  **playlists**: the Inspector compiles the best parts of each (comping), and the track header's
  menu switches between playlists.

## Recording MIDI

Enable your MIDI keyboard in Preferences › MIDI. Arm an instrument track and play: recording
creates a MIDI clip. An instrument track armed for MIDI never hears the audio inputs.
