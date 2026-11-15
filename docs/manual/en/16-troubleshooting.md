# Troubleshooting

## Common problems

| Problem | What to do |
|---|---|
| **Crackles or dropouts** | Raise the buffer size in Preferences › Audio. On Linux, run `audiostrada diagnose`. Freeze heavy tracks. Check the **DSP** meter of the transport bar. |
| **No sound** | Check the device in Preferences › Audio, that the master is not muted, and that no track is soloed. *Test tone* checks the output. |
| **No input** | Check the track's input (*In 1*), that it is armed, and its monitoring button. |
| **The recording is late** | Set the *Recording latency offset* in Preferences › General; `audiostrada latency` measures it with a cable from output 1 to input 1. |
| **Short dropouts every ten minutes** | That is the evaluation mode: enter a licence (Help › *License…*). |
| **A plugin misbehaves** | Right-click its insert › *Reload plugin*, or remove it. Plugins cannot crash AudioStrada itself. |
| **AudioStrada crashes at start because of a plugin** | Start it with `audiostrada --disable-third-party-plugins`, then remove the plugin. |
| **Blurry windows on Wayland** | Untick *Several screens* in Preferences › General. |
| **An assistant cannot connect** | Check that the robot button is green, then restart the assistant. |

## Command line

| Command | What it does |
|---|---|
| `audiostrada diagnose` | checks the system for low-latency audio and prints the fixes |
| `audiostrada devices` | lists the audio devices |
| `audiostrada tone` | plays a test tone on every output |
| `audiostrada monitor` | shows the input levels |
| `audiostrada latency` | measures the round-trip latency |
| `audiostrada plugins [--rescan]` | lists the installed plugins |
| `audiostrada play FILE…` | plays audio files or a project |
| `audiostrada render FILE… --output OUT.wav` | mixes audio files or a project into a WAV file |
| `audiostrada compare A B` | compares two audio files sample by sample (null test) |

## Reporting a problem

Start AudioStrada from a terminal with `AUDIOSTRADA_LOG=debug audiostrada` and include the output
when you write to **contact@audiostrada.com**, with your system, your audio interface and what
you did just before.

## Questions

**Can I open my projects from another DAW?** AudioStrada imports audio and MIDI files: export
your tracks as stems from the other DAW, then drop them on the timeline.

**Where are my recordings?** In the `Audio` folder of the project.

**Can I use my plugins?** Yes: CLAP, VST3, VST2 and LV2 (and AU on macOS), each in its own process.

**Does AudioStrada need the internet?** No. It goes online only to check for new versions when
you allowed it, and for TONE3000 when you browse it.

**Can I go back to an earlier version of my song?** Yes: File › *Project versions…*.

**Is my work saved if AudioStrada crashes?** Yes: it autosaves, and offers to recover the work
at the next start. It also offers to send a crash report: the version, the system, where the
error happened and the end of the log (never audio nor projects), with a comment and your email
if you want an answer. *Always send* stops the question; nothing is sent without your word.

**How do I report a bug or suggest a feature?** Help › *Report a bug or an idea…*: a title,
what happened or what you wish for, your email to be answered, and if you agree the version,
the system and the end of the log. It goes straight to the people who make AudioStrada.
