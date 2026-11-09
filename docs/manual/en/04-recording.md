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

## Raw or processed

The Inspector's **Record** row says what an armed audio track records:

- **Raw**: the input as it comes; its effects can still change afterwards.
- **Processed**: what its effects make of the input, on a track made under it, *(printed)*.
- **Both**: the raw take on the track, and the processed one on a silent track under it.

A processed take needs the input to go through the track: its monitoring must not be **Off**.

## Session health

While a track is armed, the transport bar shows:

- **Monitoring**: how long what you play takes to reach your ears, through the audio interface
  and the slowest effects heard live. Set the recording latency offset in Preferences for an
  exact figure.
- The audio dropouts since a track was armed.
- The recording time left on the disk.

When the monitoring is slow, **Reduce** chooses an audio buffer of 64 samples and turns the low
latency mode on.

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
- **Loop recording**: with the loop on ([[L]]), each pass becomes a take.

## Takes and comping

Each audio take is kept whole in a lane of its own under the track, named after its number and
the marker it starts at (*Chorus · Take 2*). What the track played before its first take gets a
lane *Before recording*. The track plays the comp, the last take at first; the lanes show once
there is a choice.

- Drag on a lane to comp that part in, or click a take to comp all of it. **Use** comps the
  whole take. Short crossfades smooth every edge.
- The ✕ deletes a lane; what the track plays of it stays.
- Click the stars to rate a take. A **!** says the input clipped or the audio dropped out while
  it was recorded; dropouts are marked in red on the clip.

MIDI loop recording merges its passes into one clip.

## Recording MIDI

Enable your MIDI keyboard in Preferences › MIDI. Arm an instrument track and play: recording
creates a MIDI clip. An instrument track armed for MIDI never hears the audio inputs.
