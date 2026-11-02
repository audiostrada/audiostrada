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
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            file => run.files.push(file.into()),
        }
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
    fn no_argument_runs_the_app() {
        assert_eq!(parse_str(""), Ok(Command::Run(RunArgs::default())));
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
}
