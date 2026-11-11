# AudioStrada: notes for coding agents

AudioStrada is a digital audio workstation for Linux, Windows and macOS, written in Rust, under
the GPL 3.0 or later. Read `README.md` for what it does, `docs/building.md` for system packages,
and `CONTRIBUTING.md` for the contribution rules. This file adds what an agent needs to work on
the code without breaking it.

## Workspace

| Crate | Role |
|---|---|
| `audiostrada-core` | Project model (`model.rs`), edits (`edit.rs`), routing, undo history, file format and migrations (`file.rs`, `legacy.rs`), temporary folders (`scratch.rs`) |
| `audiostrada-engine` | Real-time graph (`graph.rs`), compiled schedule with delay compensation (`schedule.rs`), transport, recording (`record.rs`), input taps (`tap.rs`), the project player (`project.rs`), nodes (`nodes/`) |
| `audiostrada-io` | Audio backends: PipeWire, JACK, ALSA, WASAPI, ASIO, CoreAudio, and a `dummy` one for tests |
| `audiostrada-dsp` | Built-in effects and instruments; `EFFECTS` and `EffectInfo` in `lib.rs` describe them |
| `audiostrada-media` | Decoding, encoding, resampling, peak files |
| `audiostrada-mp3` | MP3 encoder helper binary; the LGPL encoder (`shine-rs`) is linked here only |
| `audiostrada-midi` | MIDI devices, clips, files |
| `audiostrada-plugins` | CLAP, VST3 and LV2 hosting, each plugin in a sandboxed process |
| `audiostrada-ui` | egui interface: `lib.rs` (the application), `document.rs` (the open project), `actions.rs`, `keymap.rs`, `layout.rs`, `timeline/`, `mixer.rs`, editors |
| `audiostrada-license` | Licenses of the official builds |
| `audiostrada-rt` | Real-time safety tooling: channels, logger, allocation checks |
| `audiostrada-app` | The `audiostrada` binary and its command line |
| `audiostrada-test-plugin` | A minimal CLAP plugin built for the tests |

The toolchain is pinned in `rust-toolchain.toml`. Edition 2024, `rustfmt` at 100 columns.

## Build, check, test

- `cargo fmt --all`, then `cargo clippy --workspace --all-targets` with no warning (CI runs it
  with `-D warnings`).
- **Run the tests only with `scripts/test.sh`, never with a bare `cargo test`.** The suite is
  heavy (the interface tests run the whole application headless, thousands of frames each),
  and a bare run froze a developer's computer until it had to be restarted. The script runs it
  at the lowest priority, within half the processor and memory (a systemd scope on Linux), with
  `AUDIOSTRADA_NO_REALTIME=1`. Arguments go to `cargo test`:
  `scripts/test.sh -p audiostrada-engine sidechain`. Test the crates you touched first, the
  workspace (`scripts/test.sh`) once before committing, in the background. Release builds too
  stay throttled: `nice -n 19 cargo build -j 4 --release`. CI keeps a plain `cargo test`: its
  machines have no one to disturb, and the code itself keeps renders and tests off real time.
- **Light sweeps by default.** The interface tests that click everywhere (`test_sweep::script`)
  click three times sparser, and the level tests check every fourth factory sound, unless
  `AUDIOSTRADA_SWEEP=full` is set, as in CI. Set it after changing an editor's layout or a
  factory sound. Split a long test into several (`_0`, `_1`...) so that they run side by side.
- **Real time only for live audio.** A thread promoted with `audiostrada_rt::thread` runs before
  anything else on the machine, the desktop included. Only the live engine's adaptive worker
  pool, the audio backends and the plugin hosts' audio threads are promoted; renders (exports,
  tests) never are. `forbid_realtime()` or `AUDIOSTRADA_NO_REALTIME` turns promotion off, and
  a document under `cfg(test)` has no worker threads.
- CI builds and tests on Linux, Windows and macOS. Code under `cfg(windows)` or
  `cfg(target_os = "macos")` is only checked there: read the APIs you call from the crate
  sources in `~/.cargo/registry` rather than from memory.
- `cargo deny check` runs in CI (`deny.toml`): new dependencies need a GPL-compatible license.
  Never link LGPL code statically into the main binary; the MP3 encoder runs as the separate
  `audiostrada-mp3` process for this reason, and ships next to `audiostrada` in every package.

## Rules of the code

**Audio thread.** `Processor::process` and everything it calls must not allocate, lock, log
with `tracing` or make system calls. Exchange data through `audiostrada_rt::channel` (lock-free
rings), hand objects back for freeing through the engine's garbage queue, log with
`audiostrada_rt::RtLogger`. Debug builds report allocations inside `rt_section`.

**Project model and files.** New fields of `Project`, `Track` and the like take
`#[serde(default)]` (often with `skip_serializing_if`) so that older files still load. A change
that old files cannot express bumps `file::VERSION` and adds a step to `file::migrate`. Data of
optional modules goes in `Project.extensions`, which every build keeps as it is.

**Edits and undo.** Project changes go through `doc.history_mut().edit(label, |p| ...)` (one
undo step, label translated with `tr`), `edit_merged` for a continuous gesture, or
`modify_unrecorded` for settings that are not undo steps. Put the logic in
`audiostrada-core/src/edit.rs` (or `routing.rs`) and test it there.

**Engine latency.** Effects declare their latency (`Effect::latency`, and
`audiostrada_dsp::declared_latency` when it depends on parameters); the scheduler aligns every
path with delay lines. What is played live (armed tracks, tracks always hearing their input) is
never delayed by other tracks: their players are delayed instead. Input monitors are
`set_unaligned`, live tracks `set_live`.

**Commands.** Every user command is an `Action` in `actions.rs` (category and English name),
run in `AudioStradaApp::run`, listed in the menus and the command palette, and swept by
`every_command_runs_without_a_panic`. A new default shortcut goes in a preset table of
`keymap.rs` and bumps its `VERSION` (and `upgrade` if old keymaps should receive it). A new
panel is a `Pane` in `layout.rs`, added to old saved layouts in `layout::load`.

**Interface texts.** Every visible string is an English key passed to `tr("...")` or
`trf!("... {name}", name = value)`, with its French translation in
`crates/audiostrada-ui/assets/i18n/fr/*.txt`: the English line, the French line, a blank line.
`every_text_has_its_french` lists what is missing. Write plain, sentence-case texts that name
things the way users do, without jargon; French uses the same terms as the existing
translations (Piste, Prise, Écoute, Mode prise…).

**Effects.** A built-in effect has its `INFO` (parameters with stable ids, ranges, defaults) in
`EFFECTS`, a factory in `create`, an editor in the interface, factory presets, and a section in
the manual (`<!-- effect:id -->` in `docs/manual/{en,fr}`, after
`node docs/manual/build.mjs --effects`). Parameter ids are stored in projects: never rename one.

**Comments.** Few and short, in English: one or two lines saying why, not what. Documentation
comments on public items and modules, in the same plain style. No emojis, no em dashes.

## Tests

- Unit tests sit next to the code (`#[cfg(test)] mod tests`). Engine tests render through
  `EngineProcessor::render` with `ChannelBuffers`; see `project.rs` (`fixture`, `render`).
- `crates/audiostrada-ui/src/app_tests.rs` drives the whole application headless on the dummy
  backend (`Harness::new`, `with_song`, `frame`, `act`); `test_sweep.rs` scripts a user trying
  every control. Add to them when you add a command, a panel or a window.
- Tests never touch the user's files: `config_dir()` points to a temporary folder under
  `cfg(test)`, file dialogs are cancelled, the network is not reached. Temporary files go in
  `audiostrada_core::scratch::dir(name)`, under one folder per process that the next run sweeps;
  never write directly into the system's temporary folder.
- Test names are sentences saying what must hold
  (`takes_land_where_the_delayed_playback_was_heard`).

## Looking at the interface

- `AUDIOSTRADA_SCREENSHOT=out.ppm audiostrada project.strada` saves the main window after a few
  frames and quits. Combine with `XDG_CONFIG_HOME=<folder>` to start from a configuration of
  your own instead of the user's (a `Dummy` audio backend leaves the user's sound card alone;
  set `XDG_DATA_HOME`, `XDG_STATE_HOME` and `XDG_CACHE_HOME` too, or its autosaves land in the
  user's recovery folder and its log replaces theirs),
  and with `AUDIOSTRADA_SCREENSHOT_FRAME=<n>` and `--control` to set the window up through MCP
  before the shot.
- `audiostrada --control PORT` lets `audiostrada mcp --connect PORT` drive a running window
  through the Model Context Protocol (`crates/audiostrada-ui/src/control.rs` lists the tools).
- `audiostrada help` lists the command line (render, compare, latency, devices, plugin scan).

## Windowing: floating windows

AudioStrada opens windows besides the main one: detached panels (mixer, piano roll, rack),
effect editors, the effect browser, and plugin windows (built-in, third-party in process, and
third-party in a sandboxed plugin host, which is another process). They must behave the same
on every system, as Reaper's floating windows do.

### Expected behavior, on every system

- A floating window stays visible until it is closed. It never goes behind the main window.
- A detached panel never covers a window put over it, on whatever screen. For example, the
  mixer maximized on a second screen, clicked, does not hide the browser or a plugin window
  placed over it.
- Floating windows move freely from one screen to another.
- AudioStrada shows as one application in the taskbar, the Dock and Alt+Tab: floating
  windows have no entry of their own.
- Minimizing the main window hides the floating windows; restoring it brings them back.
- A window opens centered on the window it was opened from, so on that window's screen. It
  then opens where it was last left, if that is still on a screen.
- Opening a window that is already open brings it to the front. It must never get lost
  behind another window with no way to get it back.
- Floating windows never cover AudioStrada's dialogs (file choosers, messages).
- When another application is in front, floating windows do not cover it.

Test any windowing change with two screens and the mixer detached and maximized on the
second one. Open the browser and a third-party plugin from the mixer, move them over it,
click the mixer, then open them again.

When driving the real windows from a script (`SetCursorPos`, `mouse_event`), move the mouse
a few pixels before clicking: egui-winit drops a click whose window has had no cursor
position since the pointer left it, so a teleported cursor clicks into nothing and looks like
a frozen window. `GetGUIThreadInfo` (capture, active, focus windows of AudioStrada's UI
thread) and `WindowFromPoint` tell where input really goes.

### Never break the other systems

Windowing code differs on each system and each part sits behind `cfg`. A change for one
system must not alter the others. The Linux behavior (X11, and Wayland run in X11 mode) and
the macOS behavior were tuned by hand; keep them unless they are tested on those systems.

### Windows (implemented)

- **AudioStrada's own windows** (winit viewports) are owned by another window, set with
  `SetWindowLongPtrW(GWLP_HWNDPARENT)` in `own_windows`
  ([window.rs](crates/audiostrada-plugins/src/window.rs)). The UI calls it at every frame
  from `keep_windows_above` ([lib.rs](crates/audiostrada-ui/src/lib.rs)).
  - Detached panels are owned by the main window.
  - Every other window (editors, browser) is owned by the panel it overlaps most, else by the
    main window. Windows then keeps it above that panel.
- **Taskbar and Alt+Tab:** `floating_builder` sets `with_taskbar(false)` and no minimize box.
  winit also gives every window `WS_EX_APPWINDOW`, which puts even owned windows in Alt+Tab,
  and sets it again whenever a window's state changes. `own_windows` therefore removes it at
  every frame.
- **Closing a panel:** Windows destroys owned windows with their owner, behind winit's back,
  and eframe never recovers such a window. Before a panel's window closes, `release_windows`
  hands its windows to the main window (see `detached_windows`).
- **Window positions:** a position given to `ViewportBuilder` must not change from one frame
  to the next, or egui moves the window. `floating_builder` computes the centered position
  once per viewport.
- **Sandboxed plugin windows** live in the plugin host process. Never make them owned by, or
  children of, a window of AudioStrada's process: Windows would then share input between the
  two processes, and a busy plugin host would freeze AudioStrada. `HostWindow::follow` keeps
  them in place instead:
  - topmost while one of AudioStrada's winit windows, or a plugin window, is in front;
  - just behind one of AudioStrada's dialogs in front;
  - left under another application when that one is in front;
  - hidden while the main window is minimized.

  These windows use the tool window style, to stay out of the taskbar and Alt+Tab. Windows
  refuses the foreground to a background process, so AudioStrada calls `allow_foreground`
  before `OpenGui` and `RaiseGui`, and `raise` puts the window topmost by itself.
  `follow` checks the window's real topmost style rather than trusting what it last set:
  a plugin can take the style off. Some plugin processes refuse z-order changes on our frame
  window altogether once their editor was opened before (Analog Lab V: topmost, `HWND_TOP`,
  even from AudioStrada in front, all ignored; FabFilter's are fine). `follow` then retries
  twice a second at most, and clicking the plugin's slot still brings the window to the front
  (activation is not refused).
- **Plugin windows in AudioStrada's process** (sandbox off) are owned by the main window.
- **Test:** `floating_windows_are_owned_by_the_panel_under_them` in `window.rs` checks the
  ownership rules on real Win32 windows.

### Linux (X11; Wayland runs in X11 mode)

- `floating_level` sets the window levels. The browser floats; an editor floats while the
  main window, its own window or the one drawing it has the focus; panels stay at the normal
  level.
- Floating windows drop under the main window's menus, dialogs and file choosers
  (`uncover_main`).
- Plugin windows are `WM_TRANSIENT_FOR` the main window.
- Making the egui windows transient for the main window was tried and reverted (commit
  0ec8c08).

Panels can still go behind the main window here, unlike the expected behavior above. Closing
that gap needs testing under X11 and Wayland.

### macOS

AudioStrada's windows and the plugin windows float (`NSFloatingWindowLevel`) while
AudioStrada or one of its plugin hosts is the active application, and drop to the normal level
otherwise (`keep_windows_above`, `HostWindow`).

## Commits and documentation

- Commit messages: one short line saying what changed for the user, often prefixed by the area
  (`Mixer: ...`, `Tests: ...`, `CI: ...`). No issue or person mentions.
- One commit per coherent change, with its tests; never commit files you did not create on
  purpose (scratch notes, local experiments).
- User-visible changes update the manual in both languages (`docs/manual/en`, `docs/manual/fr`)
  and, when it lists the feature, `README.md`.
