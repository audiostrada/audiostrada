#!/bin/sh
# Creates or updates the repository's labels: `sh .github/labels.sh` (needs the gh CLI).
set -e
label() { gh label create "$1" --color "$2" --description "$3" --force >/dev/null && echo "$1"; }

# What it is
label "type: bug" d73a4a "Something does not work as it should"
label "type: crash" b60205 "AudioStrada closes, freezes or loses work"
label "type: regression" e11d21 "It worked in an earlier version"
label "type: feature" a2eeef "Something new"
label "type: enhancement" 84b6eb "Better behaviour of something that exists"
label "type: performance" fbca04 "CPU, latency, dropouts, start time, memory"
label "type: plugin compatibility" c5def5 "A third-party CLAP, VST3, LV2 or AU plugin"
label "type: docs" 0075ca "Manual, guides, site texts"
label "type: translation" bfdadc "Interface texts and their translations"
label "type: question" d876e3 "A question rather than a problem"
label "type: chore" ededed "Build, CI, dependencies, cleanup"

# Where
label "area: audio engine" 1d76db "Engine, graph, real-time, devices and backends"
label "area: recording" 1d76db "Inputs, monitoring, takes, punch, loop recording"
label "area: timeline" 1d76db "Clips, tools, editing, fades, waveforms"
label "area: midi" 1d76db "Piano roll, MIDI clips, controllers, MPE"
label "area: mixer" 1d76db "Strips, sends, groups, VCAs, console, automation"
label "area: effects" 1d76db "The built-in Strada effects"
label "area: instruments" 1d76db "Strada-Synth and Strada-Drums"
label "area: plugin hosting" 1d76db "Scanning, sandbox, windows of third-party plugins"
label "area: export" 1d76db "Export, render queue, freeze, bounce"
label "area: ai assistants" 1d76db "MCP server and the assistants window"
label "area: interface" 1d76db "Windows, themes, layout, shortcuts, accessibility"
label "area: packaging" 1d76db "Installers, packages, signing, updates"
label "area: license" 1d76db "Evaluation, licences, subscriptions"

# Systems
label "os: linux" 0e8a16 "Linux (PipeWire, JACK, ALSA)"
label "os: windows" 0e8a16 "Windows (WASAPI, ASIO)"
label "os: macos" 0e8a16 "macOS (CoreAudio, AU)"

# Where it stands
label "status: triage" fef2c0 "New, not looked at yet"
label "status: needs info" fbca04 "Waiting for details from the reporter"
label "status: confirmed" 0e8a16 "Reproduced or accepted"
label "status: in progress" 5319e7 "Someone is working on it"
label "status: blocked" b60205 "Waits on something else"

# How urgent
label "priority: critical" b60205 "Data loss, crash on start, no sound"
label "priority: high" d93f0b "Important, soon"
label "priority: low" c2e0c6 "Nice to have"

# Contributors
label "good first issue" 7057ff "A good start for a new contributor"
label "help wanted" 008672 "Help from outside is welcome"
label "duplicate" cfd3d7 "Already reported"
label "wontfix" ffffff "Will not be done"
label "invalid" e4e669 "Not a problem of AudioStrada"
label "dependencies" 0366d6 "Dependency updates"
label "accessibility" f143ab "Barrier for people with disabilities"
label "breaking" b60205 "Changes project files, presets or behaviour users rely on"
