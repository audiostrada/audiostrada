use std::path::PathBuf;

use audiostrada_io::{BackendKind, StreamConfig};

pub const USAGE: &str = "\
Usage: audiostrada [WINDOW OPTIONS] [FILE...]
                                   Open the main window with a project or audio files
       audiostrada <COMMAND> [OPTIONS]

Window options:
  --control <PORT>    Let `audiostrada mcp --connect <PORT>` drive this window
  --disable-third-party-plugins
                      Neither scan nor load CLAP, VST3, LV2 or AU plugins (built-in effects stay)
  --fresh             Start as on the very first launch, with empty settings: your own are
                      neither read nor changed, and these are thrown away at the next start
  --profile <DIR>     Keep the settings, presets, caches and log in DIR instead of your own

Commands:
  diagnose           Check the system for low-latency audio and print fixes
  devices            List audio devices
  tone               Play a test tone on every output
  monitor            Show input levels
  play FILE...       Play audio files (or a .strada project) together
  render FILE...     Mix audio files (or a .strada project) into a WAV file (requires --output)
  latency            Measure the round-trip latency (loop output 1 back to input 1)
  compare A B        Compare two audio files sample by sample (null test)
  plugins [--rescan] List the installed plugins (CLAP, VST3, LV2, AU), scanning new ones
  plugin-params NAME List a plugin's parameters (by key or name), to set it for measure
  measure FILE.toml [--out DIR]
                     Fingerprint every reference of the file (consoles, effects, plugins):
                     gain, harmonics, intermodulation, memory, noise; a JSON file each
  measure --console NAME [--amount 0-100] [--bus] [--noise] [--out FILE]
                     Fingerprint a built-in console
  measure-report FILE.json|DIR... --output PAGE.html
                     Compare fingerprints (or every one of a folder) on one page of charts
  measure-dynamics FILE.toml [--out DIR]
                     Measure compressors set as the file says: static curve and harmonics,
                     response by frequency while compressing, attack and release after steps
                     held for different times; a JSON file each
  measure-presets --audio DIR [EFFECT...] [--only TEXT] [--write DIR] [--presets FILE.json]
                     Play every factory preset on a sound of its kind (vocal.wav, kick.wav,
                     mix.wav... of DIR) and on a tone: loudness, peaks, gain reduction, change by
                     band, tail, modulation
  measure-instrument SOURCE... [--out DIR] [--wav DIR] [--track NAME] [--name NAME]
                     [--keys 28-100/7] [--velocities 20,45,70,95,120] [--hold S]
                     [--set PARAM=VALUE]...
                     Play test notes through instruments (a .strada project, its track holding
                     the instrument set up by hand, or a built-in one by id) and fingerprint
                     them: pitch, partials and their decay, loudness and brightness by
                     velocity, attack, release; a JSON file each, and with --wav what was
                     played and a phrase for listening, at the same level
  measure-instrument-report FILE.json|DIR... --output PAGE.html
                     Compare instrument fingerprints on one page of charts
  measure-presets --dump [EFFECT...]
                     Print the factory presets as a file for --presets
  measure-presets --audio DIR --plugin NAME [--source SOUND] [--set PARAM=VALUE]...
                     The same measures of a plugin taken as a reference
  mcp                Serve the Model Context Protocol on stdio, for agents, on a project of its
                     own; with --connect <PORT>, drive the AudioStrada started with --control <PORT>
  license keygen     Print a new key pair for signing licenses (keep the secret private)
  license issue --secret-file <FILE> --name <NAME> --email <EMAIL> --until <YYYY-MM-DD> [--commercial]
                     Print a license signed with the secret key
  license check FILE --key <PUBLIC>
                     Check a license against a public key (hex) and print what it holds
  help               Show this help

Audio options (devices, tone, monitor, play, render, latency):
  --backend <NAME>    pipewire, jack, alsa, wasapi, asio, coreaudio, dummy (default: auto)
  --device <ID>       Input and output device, from `audiostrada devices`
  --input-device <ID>, --output-device <ID>
  --rate <HZ>         Sample rate (default: device rate; render: 48000)
  --buffer <FRAMES>   Buffer size (default: 256)
  --inputs <N>        Input channels (default: 2)
  --outputs <N>       Output channels (default: 2)
  --seconds <S>       Duration (default: 5; play: length of the longest file)
  --freq <HZ>         Test tone frequency (default: 440)
  --loop              Loop playback over the whole session (play)
  --output <FILE>     Destination WAV file (render)
  --threshold <0-1>   Detection level for latency (default: 0.1)

Environment:
  AUDIOSTRADA_LOG          Log filter, e.g. debug or audiostrada::rt=trace";

#[derive(Debug, PartialEq)]
pub enum Command {
    Run(RunArgs),
    Help,
    Diagnose,
    Devices(AudioArgs),
    Tone(AudioArgs),
    Monitor(AudioArgs),
    Play(AudioArgs),
    Render(AudioArgs),
    Latency(AudioArgs),
    Compare(AudioArgs),
    Plugins {
        rescan: bool,
    },
    /// Internal: scans one plugin file and prints JSON (run in a child process).
    PluginScan(String, PathBuf),
    /// Internal: hosts one plugin for a sandboxed instance (run in a child process).
    PluginHost,
    /// MCP server on stdio, headless or connected to a running AudioStrada.
    Mcp {
        connect: Option<u16>,
    },
    License(LicenseCommand),
    Measure(MeasureArgs),
    MeasureReport {
        files: Vec<PathBuf>,
        output: PathBuf,
    },
    MeasureDynamics {
        references: PathBuf,
        out: PathBuf,
    },
    PluginParams(String),
    MeasurePresets(crate::preset_report::Args),
    MeasureInstrument(crate::instrument::Args),
    MeasureInstrumentReport {
        files: Vec<PathBuf>,
        output: PathBuf,
    },
}

/// What `measure` fingerprints: the references of a file, or one built-in console.
#[derive(Debug, Default, PartialEq)]
pub struct MeasureArgs {
    pub references: Option<PathBuf>,
    pub console: Option<String>,
    pub amount: Option<f32>,
    pub bus: bool,
    pub noise: bool,
    pub out: Option<PathBuf>,
}

#[derive(Debug, PartialEq)]
pub enum LicenseCommand {
    Keygen,
    Check {
        file: PathBuf,
        key: String,
    },
    Issue {
        secret_file: PathBuf,
        name: String,
        email: String,
        until: String,
        commercial: bool,
    },
}

/// The window, with files to open.
#[derive(Debug, Default, PartialEq)]
pub struct RunArgs {
    pub files: Vec<PathBuf>,
    /// Port of the control channel.
    pub control: Option<u16>,
    pub disable_plugins: bool,
    /// A new, empty profile for this run only.
    pub fresh: bool,
    /// A profile folder in place of the user's own.
    pub profile: Option<PathBuf>,
}

#[derive(Debug, PartialEq)]
pub struct AudioArgs {
    pub backend: Option<BackendKind>,
    pub config: StreamConfig,
    pub seconds: Option<f64>,
    pub frequency: f32,
    pub files: Vec<PathBuf>,
    pub looping: bool,
    pub output: Option<PathBuf>,
    pub threshold: f32,
}

impl Default for AudioArgs {
    fn default() -> Self {
        Self {
            backend: None,
            config: StreamConfig::default(),
            seconds: None,
            frequency: 440.0,
            files: Vec::new(),
            looping: false,
            output: None,
            threshold: 0.1,
        }
    }
}

pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let Some(command) = args.next() else {
        return Ok(Command::Run(RunArgs::default()));
    };
    let wrap: fn(AudioArgs) -> Command = match command.as_str() {
        "help" | "-h" | "--help" => return Ok(Command::Help),
        "diagnose" => return Ok(Command::Diagnose),
        "plugins" => {
            let mut rescan = false;
            for arg in args {
                match arg.as_str() {
                    "--rescan" => rescan = true,
                    other => return Err(format!("unknown option '{other}' for plugins")),
                }
            }
            return Ok(Command::Plugins { rescan });
        }
        "plugin-host" => return Ok(Command::PluginHost),
        "mcp" => {
            let mut connect = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--connect" => connect = Some(port(args.next(), "--connect")?),
                    other => return Err(format!("unknown option '{other}' for mcp")),
                }
            }
            return Ok(Command::Mcp { connect });
        }
        "license" => return parse_license(args),
        "measure" => return parse_measure(args),
        "measure-report" => {
            let (mut files, mut output) = (Vec::new(), None);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--output" => output = args.next().map(PathBuf::from),
                    other if other.starts_with("--") => {
                        return Err(format!("unknown option '{other}' for measure-report"));
                    }
                    _ => files.push(arg.into()),
                }
            }
            return match (files.is_empty(), output) {
                (false, Some(output)) => Ok(Command::MeasureReport { files, output }),
                _ => Err("'measure-report' needs fingerprints and --output <FILE>".into()),
            };
        }
        "measure-dynamics" => {
            let (mut references, mut out) = (None, None);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--out" => out = args.next().map(PathBuf::from),
                    other if other.starts_with("--") => {
                        return Err(format!("unknown option '{other}' for measure-dynamics"));
                    }
                    _ if references.is_none() => references = Some(arg.into()),
                    _ => return Err("'measure-dynamics' takes one references file".into()),
                }
            }
            return match references {
                Some(references) => Ok(Command::MeasureDynamics {
                    references,
                    out: out.unwrap_or_else(|| "dynamics".into()),
                }),
                None => Err("'measure-dynamics' needs a references file".into()),
            };
        }
        "measure-instrument" => return parse_measure_instrument(args),
        "measure-instrument-report" => {
            let (mut files, mut output) = (Vec::new(), None);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--output" => output = args.next().map(PathBuf::from),
                    other if other.starts_with("--") => {
                        return Err(format!(
                            "unknown option '{other}' for measure-instrument-report"
                        ));
                    }
                    _ => files.push(arg.into()),
                }
            }
            return match (files.is_empty(), output) {
                (false, Some(output)) => Ok(Command::MeasureInstrumentReport { files, output }),
                _ => {
                    Err("'measure-instrument-report' needs fingerprints and --output <FILE>".into())
                }
            };
        }
        "measure-presets" => {
            let mut m = crate::preset_report::Args::default();
            let mut audio = None;
            while let Some(arg) = args.next() {
                if !arg.starts_with("--") {
                    m.effects.push(arg);
                    continue;
                }
                if arg == "--dump" {
                    m.dump = true;
                    continue;
                }
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {arg}"))?;
                match arg.as_str() {
                    "--audio" => audio = Some(PathBuf::from(value)),
                    "--only" => m.only = Some(value),
                    "--write" => m.write = Some(value.into()),
                    "--plugin" => m.plugin = Some(value),
                    "--source" => m.source = Some(value),
                    "--presets" => m.presets = Some(value.into()),
                    "--set" => {
                        let pair = value.rsplit_once('=').and_then(|(k, v)| {
                            v.trim().parse().ok().map(|v| (k.trim().to_owned(), v))
                        });
                        m.set
                            .push(pair.ok_or_else(|| format!("invalid setting '{value}'"))?);
                    }
                    other => return Err(format!("unknown option '{other}' for measure-presets")),
                }
            }
            m.audio = match audio {
                Some(audio) => audio,
                None if m.dump => PathBuf::new(),
                None => return Err("'measure-presets' needs --audio <DIR>".into()),
            };
            return Ok(Command::MeasurePresets(m));
        }
        "plugin-params" => {
            let query: Vec<String> = args.collect();
            if query.is_empty() {
                return Err("'plugin-params' needs a plugin's key or name".into());
            }
            return Ok(Command::PluginParams(query.join(" ")));
        }
        "plugin-scan" => {
            let (Some(format), Some(path)) = (args.next(), args.next()) else {
                return Err("'plugin-scan' needs a format and a file".into());
            };
            return Ok(Command::PluginScan(format, path.into()));
        }
        "devices" => Command::Devices,
        "tone" => Command::Tone,
        "monitor" => Command::Monitor,
        "play" => Command::Play,
        "render" => Command::Render,
        "latency" => Command::Latency,
        "compare" => Command::Compare,
        _ => return parse_run(std::iter::once(command.clone()).chain(args)),
    };
    let mut audio = AudioArgs::default();
    while let Some(arg) = args.next() {
        if !arg.starts_with("--") {
            audio.files.push(arg.into());
            continue;
        }
        if arg == "--loop" {
            audio.looping = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {arg}"))?;
        let invalid = || format!("invalid value '{value}' for {arg}");
        match arg.as_str() {
            "--backend" => {
                audio.backend = Some(BackendKind::from_name(&value).ok_or_else(invalid)?);
            }
            "--device" => {
                audio.config.input_device = Some(value.clone());
                audio.config.output_device = Some(value);
            }
            "--input-device" => audio.config.input_device = Some(value),
            "--output-device" => audio.config.output_device = Some(value),
            "--rate" => audio.config.sample_rate = Some(value.parse().map_err(|_| invalid())?),
            "--buffer" => audio.config.buffer_size = value.parse().map_err(|_| invalid())?,
            "--inputs" => audio.config.inputs = value.parse().map_err(|_| invalid())?,
            "--outputs" => audio.config.outputs = value.parse().map_err(|_| invalid())?,
            "--seconds" => audio.seconds = Some(value.parse().map_err(|_| invalid())?),
            "--freq" => audio.frequency = value.parse().map_err(|_| invalid())?,
            "--output" => audio.output = Some(value.into()),
            "--threshold" => audio.threshold = value.parse().map_err(|_| invalid())?,
            _ => return Err(format!("unknown option '{arg}'")),
        }
    }
    let command = wrap(audio);
    match &command {
        Command::Play(a) if a.files.is_empty() => {
            Err("'play' needs at least one audio file".into())
        }
        Command::Render(a) if a.files.is_empty() => {
            Err("'render' needs at least one audio file".into())
        }
        Command::Render(a) if a.output.is_none() => Err("'render' needs --output <FILE>".into()),
        Command::Compare(a) if a.files.len() != 2 => {
            Err("'compare' needs exactly two files".into())
        }
        _ => Ok(command),
    }
}

fn parse_measure_instrument(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut m = crate::instrument::Args::default();
    while let Some(arg) = args.next() {
        if !arg.starts_with("--") {
            m.sources.push(arg);
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {arg}"))?;
        let invalid = || format!("invalid value '{value}' for {arg}");
        match arg.as_str() {
            "--out" => m.out = value.into(),
            "--wav" => m.wav = Some(value.into()),
            "--track" => m.track = Some(value),
            "--name" => m.name = Some(value),
            "--hold" => m.hold = value.parse().map_err(|_| invalid())?,
            "--keys" => {
                // FIRST-LAST/STEP, or keys apart by commas.
                m.keys = if let Some((range, step)) = value.split_once('/') {
                    let (a, b) = range.split_once('-').ok_or_else(invalid)?;
                    let (a, b, step): (u8, u8, usize) = (
                        a.parse().map_err(|_| invalid())?,
                        b.parse().map_err(|_| invalid())?,
                        step.parse().map_err(|_| invalid())?,
                    );
                    (a..=b).step_by(step.max(1)).collect()
                } else {
                    numbers(&value).ok_or_else(invalid)?
                };
            }
            "--velocities" => m.velocities = numbers(&value).ok_or_else(invalid)?,
            "--set" => {
                let pair = value
                    .rsplit_once('=')
                    .and_then(|(k, v)| v.trim().parse().ok().map(|v| (k.trim().to_owned(), v)));
                m.set.push(pair.ok_or_else(invalid)?);
            }
            other => return Err(format!("unknown option '{other}' for measure-instrument")),
        }
    }
    if m.sources.is_empty() {
        return Err("'measure-instrument' needs a project or a built-in instrument".into());
    }
    if m.name.is_some() && m.sources.len() > 1 {
        return Err("'--name' names one source's fingerprint".into());
    }
    if m.keys.is_empty() || m.velocities.is_empty() || m.hold <= 0.0 {
        return Err("'measure-instrument' needs keys, velocities and a hold time".into());
    }
    Ok(Command::MeasureInstrument(m))
}

/// Numbers apart by commas, each from 1 to 127.
fn numbers(text: &str) -> Option<Vec<u8>> {
    text.split(',')
        .map(|v| v.trim().parse().ok().filter(|v| (1..=127).contains(v)))
        .collect()
}

fn parse_measure(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut m = MeasureArgs::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("missing value for {name}"))
        };
        match arg.as_str() {
            "--console" => m.console = Some(value("--console")?),
            "--amount" => {
                let v = value("--amount")?;
                m.amount = Some(v.parse().map_err(|_| format!("invalid amount '{v}'"))?);
            }
            "--out" => m.out = Some(value("--out")?.into()),
            "--bus" => m.bus = true,
            "--noise" => m.noise = true,
            other if other.starts_with("--") => {
                return Err(format!("unknown option '{other}' for measure"));
            }
            _ if m.references.is_none() => m.references = Some(arg.into()),
            _ => return Err("'measure' takes one references file".into()),
        }
    }
    if m.references.is_some() == m.console.is_some() {
        return Err("'measure' needs a references file or --console <NAME>".into());
    }
    Ok(Command::Measure(m))
}

fn parse_license(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    match args.next().as_deref() {
        Some("keygen") => Ok(Command::License(LicenseCommand::Keygen)),
        Some("check") => match (args.next(), args.next().as_deref(), args.next()) {
            (Some(file), Some("--key"), Some(key)) => Ok(Command::License(LicenseCommand::Check {
                file: file.into(),
                key,
            })),
            _ => Err("'license check' needs a file and --key <PUBLIC>".into()),
        },
        Some("issue") => {
            let (mut secret_file, mut name, mut email, mut until) = (None, None, None, None);
            let mut commercial = false;
            while let Some(arg) = args.next() {
                if arg == "--commercial" {
                    commercial = true;
                    continue;
                }
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {arg}"))?;
                match arg.as_str() {
                    "--secret-file" => secret_file = Some(PathBuf::from(value)),
                    "--name" => name = Some(value),
                    "--email" => email = Some(value),
                    "--until" => until = Some(value),
                    other => return Err(format!("unknown option '{other}' for license issue")),
                }
            }
            match (secret_file, name, email, until) {
                (Some(secret_file), Some(name), Some(email), Some(until)) => {
                    Ok(Command::License(LicenseCommand::Issue {
                        secret_file,
                        name,
                        email,
                        until,
                        commercial,
                    }))
                }
                _ => Err("'license issue' needs --secret-file, --name, --email and --until".into()),
            }
        }
        _ => Err("'license' needs 'keygen', 'issue' or 'check'".into()),
    }
}

fn parse_run(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut run = RunArgs::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--control" => run.control = Some(port(args.next(), "--control")?),
            "--disable-third-party-plugins" => run.disable_plugins = true,
            "--fresh" => run.fresh = true,
            "--profile" => {
                let dir = args.next().ok_or("--profile needs a folder")?;
                run.profile = Some(dir.into());
            }
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            file => run.files.push(file.into()),
        }
    }
    if run.fresh && run.profile.is_some() {
        return Err("--fresh and --profile cannot go together".into());
    }
    Ok(Command::Run(run))
}

fn port(value: Option<String>, option: &str) -> Result<u16, String> {
    let value = value.ok_or_else(|| format!("{option} needs a port"))?;
    value.parse().map_err(|_| format!("invalid port '{value}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Command, String> {
        parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn measure_instrument_takes_its_sources_keys_and_settings() {
        let Ok(Command::MeasureInstrument(m)) = parse_str(
            "measure-instrument ep80 song.strada --keys 40-64/12 --velocities 30,90 --set hammer=70",
        ) else {
            panic!("not parsed");
        };
        assert_eq!(m.sources, ["ep80", "song.strada"]);
        assert_eq!(m.keys, [40, 52, 64]);
        assert_eq!(m.velocities, [30, 90]);
        assert_eq!(m.set, [("hammer".to_owned(), 70.0)]);
        assert!(parse_str("measure-instrument").is_err());
        assert!(parse_str("measure-instrument ep80 --velocities 0").is_err());
    }

    #[test]
    fn no_argument_runs_the_app() {
        assert_eq!(parse_str(""), Ok(Command::Run(RunArgs::default())));
    }

    #[test]
    fn a_fresh_start_or_a_profile_is_chosen_but_not_both() {
        assert_eq!(
            parse_str("--fresh song.strada"),
            Ok(Command::Run(RunArgs {
                fresh: true,
                files: vec!["song.strada".into()],
                ..RunArgs::default()
            }))
        );
        assert_eq!(
            parse_str("--profile /tmp/tester"),
            Ok(Command::Run(RunArgs {
                profile: Some("/tmp/tester".into()),
                ..RunArgs::default()
            }))
        );
        assert!(parse_str("--profile").is_err());
        assert!(parse_str("--fresh --profile /tmp/tester").is_err());
    }

    #[test]
    fn bare_files_open_the_app() {
        assert_eq!(
            parse_str("a.wav b.flac"),
            Ok(Command::Run(RunArgs {
                files: vec!["a.wav".into(), "b.flac".into()],
                ..RunArgs::default()
            }))
        );
    }

    #[test]
    fn window_options_come_in_any_order() {
        assert_eq!(
            parse_str("--disable-third-party-plugins --control 9000 song.strada"),
            Ok(Command::Run(RunArgs {
                files: vec!["song.strada".into()],
                control: Some(9000),
                disable_plugins: true,
                ..RunArgs::default()
            }))
        );
        assert!(parse_str("--control").is_err());
        assert!(parse_str("a.wav --dance").is_err());
    }

    #[test]
    fn audio_options_are_parsed() {
        let Ok(Command::Tone(args)) =
            parse_str("tone --backend dummy --buffer 128 --rate 44100 --seconds 2 --freq 1000")
        else {
            panic!("expected tone command");
        };
        assert_eq!(args.backend, Some(BackendKind::Dummy));
        assert_eq!(args.config.buffer_size, 128);
        assert_eq!(args.config.sample_rate, Some(44_100));
        assert_eq!(args.seconds, Some(2.0));
        assert_eq!(args.frequency, 1000.0);
    }

    #[test]
    fn play_and_render_take_files() {
        let Ok(Command::Play(args)) = parse_str("play drums.wav --loop bass.flac") else {
            panic!("expected play command");
        };
        assert_eq!(
            args.files,
            vec![PathBuf::from("drums.wav"), PathBuf::from("bass.flac")]
        );
        assert!(args.looping);
        let Ok(Command::Render(args)) = parse_str("render a.wav --output mix.wav") else {
            panic!("expected render command");
        };
        assert_eq!(args.output, Some(PathBuf::from("mix.wav")));
    }

    #[test]
    fn errors_are_reported() {
        assert!(parse_str("--dance").is_err());
        assert!(parse_str("tone --buffer").is_err());
        assert!(parse_str("tone --buffer big").is_err());
        assert!(parse_str("tone --backend nope").is_err());
        assert!(parse_str("monitor --volume 3").is_err());
        assert!(parse_str("play").is_err());
        assert_eq!(
            parse_str("render --output mix.wav"),
            Err("'render' needs at least one audio file".into())
        );
        assert!(parse_str("render a.wav").is_err());
        assert!(parse_str("compare a.wav").is_err());
        assert!(parse_str("compare a.wav b.wav").is_ok());
    }

    #[test]
    fn device_and_channel_options_fill_the_stream_config() {
        let Ok(Command::Monitor(args)) = parse_str("monitor --device hw:1 --inputs 4 --outputs 0")
        else {
            panic!("expected monitor command");
        };
        assert_eq!(args.config.input_device.as_deref(), Some("hw:1"));
        assert_eq!(args.config.output_device.as_deref(), Some("hw:1"));
        assert_eq!((args.config.inputs, args.config.outputs), (4, 0));
        let Ok(Command::Latency(args)) =
            parse_str("latency --input-device a --output-device b --threshold 0.25")
        else {
            panic!("expected latency command");
        };
        assert_eq!(args.config.input_device.as_deref(), Some("a"));
        assert_eq!(args.config.output_device.as_deref(), Some("b"));
        assert_eq!(args.threshold, 0.25);
        assert_eq!(args.frequency, 440.0, "default");
    }

    #[test]
    fn every_audio_command_is_recognised() {
        for (text, name) in [
            ("devices", "Devices"),
            ("tone", "Tone"),
            ("monitor", "Monitor"),
            ("latency", "Latency"),
        ] {
            let command = parse_str(text).unwrap();
            assert!(format!("{command:?}").starts_with(name), "{command:?}");
        }
        assert_eq!(parse_str("help"), Ok(Command::Help));
        assert_eq!(parse_str("--help"), Ok(Command::Help));
        assert_eq!(parse_str("-h"), Ok(Command::Help));
        assert_eq!(parse_str("diagnose"), Ok(Command::Diagnose));
        assert_eq!(parse_str("plugin-host"), Ok(Command::PluginHost));
    }

    #[test]
    fn plugin_commands() {
        assert_eq!(parse_str("plugins"), Ok(Command::Plugins { rescan: false }));
        assert_eq!(
            parse_str("plugins --rescan"),
            Ok(Command::Plugins { rescan: true })
        );
        assert!(parse_str("plugins --rescna").is_err());
        assert_eq!(
            parse_str("plugin-scan clap /a/b.clap"),
            Ok(Command::PluginScan("clap".into(), "/a/b.clap".into()))
        );
        assert!(parse_str("plugin-scan clap").is_err());
    }

    #[test]
    fn mcp_runs_headless_or_connects() {
        assert_eq!(parse_str("mcp"), Ok(Command::Mcp { connect: None }));
        assert_eq!(
            parse_str("mcp --connect 9000"),
            Ok(Command::Mcp {
                connect: Some(9000)
            })
        );
        assert_eq!(
            parse_str("mcp --connect"),
            Err("--connect needs a port".into())
        );
        assert!(parse_str("mcp --connect 70000").is_err());
        assert!(parse_str("mcp --connect 9000 extra").is_err());
        assert!(parse_str("mcp --port 9000").is_err());
    }

    #[test]
    fn measure_takes_a_references_file_or_a_console() {
        assert_eq!(
            parse_str("measure refs.toml --out prints"),
            Ok(Command::Measure(MeasureArgs {
                references: Some("refs.toml".into()),
                out: Some("prints".into()),
                ..MeasureArgs::default()
            }))
        );
        assert_eq!(
            parse_str("measure --console Nashville --amount 70 --bus"),
            Ok(Command::Measure(MeasureArgs {
                console: Some("Nashville".into()),
                amount: Some(70.0),
                bus: true,
                ..MeasureArgs::default()
            }))
        );
        assert!(parse_str("measure").is_err());
        assert_eq!(
            parse_str("measure-presets --audio sounds eq --only vocal --set mix=50"),
            Ok(Command::MeasurePresets(crate::preset_report::Args {
                audio: "sounds".into(),
                effects: vec!["eq".into()],
                only: Some("vocal".into()),
                set: vec![("mix".into(), 50.0)],
                ..Default::default()
            }))
        );
        assert!(parse_str("measure-presets eq").is_err());
        assert!(parse_str("measure a.toml --console X").is_err());
        assert_eq!(
            parse_str("measure-report a.json b.json --output page.html"),
            Ok(Command::MeasureReport {
                files: vec!["a.json".into(), "b.json".into()],
                output: "page.html".into(),
            })
        );
        assert!(parse_str("measure-report a.json").is_err());
        assert_eq!(
            parse_str("plugin-params Pro-Q 4"),
            Ok(Command::PluginParams("Pro-Q 4".into()))
        );
    }
}
