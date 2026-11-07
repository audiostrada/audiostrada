use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use audiostrada_io::ChannelBuffers;
use audiostrada_io::diagnostics::{self, CheckStatus};
use audiostrada_io::tools::{LatencyProbe, Meters, TestTone, gain_to_db};
use audiostrada_io::{
    AudioBackend, AudioBuffers, AudioStream, CallbackTime, auto_backend, backend, open_stream,
};
use audiostrada_media::{AudioData, write_wav};
use audiostrada_plugins::scan::Database;
use audiostrada_ui::document::{AppDirs, Document};

use crate::cli::AudioArgs;

const DEFAULT_SECONDS: f64 = 5.0;
const LOOP_SECONDS: f64 = 30.0;
const RENDER_RATE: u32 = 48_000;
const RENDER_BLOCK: usize = 1024;
const METER_WIDTH: usize = 40;
const METER_FLOOR_DB: f32 = -60.0;

pub fn diagnose() {
    let checks = diagnostics::run();
    if checks.is_empty() {
        println!("No checks available on this platform yet.");
    }
    for check in checks {
        println!("[{:^7}] {}: {}", check.status, check.name, check.detail);
        if let Some(fix) = check.fix.filter(|_| check.status != CheckStatus::Ok) {
            for line in fix.lines() {
                println!("           {line}");
            }
        }
    }
}

pub fn devices(args: &AudioArgs) -> Result<()> {
    let backend = open_backend(args)?;
    println!("{} devices:", backend.kind());
    for d in backend.devices()? {
        let default = match (d.is_default_input, d.is_default_output) {
            (true, true) => " (default input and output)",
            (true, false) => " (default input)",
            (false, true) => " (default output)",
            _ => "",
        };
        println!(
            "  {}  {}  in:{} out:{}{default}",
            d.id, d.name, d.inputs, d.outputs
        );
    }
    Ok(())
}

pub fn tone(args: &AudioArgs) -> Result<()> {
    let backend = open_backend(args)?;
    let rate_hint = args.config.sample_rate.unwrap_or(48_000);
    let frequency = args.frequency;
    let mut tone = TestTone::new(frequency, -18.0, rate_hint);
    let mut rate = rate_hint;
    let callback = move |buffers: &mut AudioBuffers<'_>, time: &CallbackTime| {
        if time.sample_rate != rate {
            rate = time.sample_rate;
            tone.set_sample_rate(frequency, rate);
        }
        if buffers.output_channels() == 0 {
            return;
        }
        tone.fill(buffers.output(0));
        for channel in 1..buffers.output_channels() {
            buffers.copy_output(0, channel);
        }
    };
    let stream = open_stream(backend.as_ref(), &args.config, Box::new(callback))?;
    print_stream(stream.as_ref());
    let seconds = args.seconds.unwrap_or(DEFAULT_SECONDS);
    println!("Playing {frequency} Hz at -18 dBFS for {seconds} s");
    thread::sleep(Duration::from_secs_f64(seconds));
    print_status(stream.as_ref());
    Ok(())
}

pub fn monitor(args: &AudioArgs) -> Result<()> {
    let backend = open_backend(args)?;
    let meters = Meters::new(args.config.inputs as usize);
    let callback = {
        let meters = meters.clone();
        move |buffers: &mut AudioBuffers<'_>, _: &CallbackTime| {
            for channel in 0..buffers.input_channels().min(meters.channels()) {
                meters.update(channel, buffers.input(channel));
            }
            buffers.clear_outputs();
        }
    };
    let stream = open_stream(backend.as_ref(), &args.config, Box::new(callback))?;
    print_stream(stream.as_ref());
    let end = Instant::now() + Duration::from_secs_f64(args.seconds.unwrap_or(DEFAULT_SECONDS));
    let mut stdout = std::io::stdout();
    while Instant::now() < end {
        thread::sleep(Duration::from_millis(100));
        let mut line = String::from("\r");
        for channel in 0..meters.channels() {
            let db = gain_to_db(meters.take_peak(channel)).max(METER_FLOOR_DB);
            line += &format!("in{}: [{}] {:>6.1} dB  ", channel + 1, meter_bar(db), db);
        }
        let _ = write!(stdout, "{line}");
        let _ = stdout.flush();
    }
    println!();
    print_status(stream.as_ref());
    Ok(())
}

/// A level bar of `METER_WIDTH` characters, full at 0 dBFS and above.
fn meter_bar(db: f32) -> String {
    let filled = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB * METER_WIDTH as f32).max(0.0) as usize;
    format!("{:<METER_WIDTH$}", "#".repeat(filled.min(METER_WIDTH)))
}

fn open_backend(args: &AudioArgs) -> Result<Box<dyn AudioBackend>> {
    Ok(match args.backend {
        Some(kind) => backend(kind)?,
        None => auto_backend()?,
    })
}

fn print_stream(stream: &dyn AudioStream) {
    let info = stream.info();
    println!(
        "{} on {}: {} Hz, {} frames ({:.1} ms), {} in / {} out",
        info.backend,
        info.device,
        info.sample_rate,
        info.buffer_size,
        info.buffer_latency_ms(),
        info.inputs,
        info.outputs
    );
}

fn print_status(stream: &dyn AudioStream) {
    let status = stream.status();
    println!(
        "Actual buffer: {} frames at {} Hz. {} cycles, {} xruns, {} allocations on the audio thread{}",
        status.current_buffer_size(),
        status.current_sample_rate(),
        status.cycles(),
        status.xruns(),
        status.alloc_violations(),
        if status.panicked() {
            ", callback panicked"
        } else {
            ""
        }
    );
}

const LOAD_TIMEOUT: Duration = Duration::from_secs(600);

pub fn is_project(path: &Path) -> bool {
    audiostrada_core::file::is_project(path)
}

/// The user's settings for third-party plugins (sandbox, on or off, crash guard) put in
/// place, and the plugins found by the last scan: to give a project before it is opened, so
/// that its plugins load at once instead of being reported as not installed.
pub fn plugin_database(ui: &audiostrada_ui::UiPrefs, disabled: bool) -> Database {
    ui.apply_sandbox();
    if let Some(dir) = dirs::data_dir() {
        audiostrada_plugins::guard::set_dir(dir.join("audiostrada"));
    }
    audiostrada_plugins::set_enabled(!disabled && !ui.disable_plugins);
    crate::plugins::database_path()
        .map(|cache| Database::load(&cache))
        .unwrap_or_default()
}

/// Opens a project file, or a new project with every audio file imported as a track, with
/// these plugins installed (see [`plugin_database`]).
pub fn load_document(
    files: &[impl AsRef<Path>],
    dirs: AppDirs,
    plugins: Database,
) -> Result<Document> {
    let files: Vec<PathBuf> = files.iter().map(|f| f.as_ref().to_path_buf()).collect();
    let mut doc = match files.iter().find(|f| is_project(f)) {
        Some(project) => Document::open(project, dirs)
            .with_context(|| format!("cannot open {}", project.display()))?,
        None => Document::new(dirs),
    };
    doc.set_plugins(plugins);
    let audio: Vec<PathBuf> = files.into_iter().filter(|f| !is_project(f)).collect();
    doc.import(&audio);
    if !doc.wait_loaded(LOAD_TIMEOUT) {
        anyhow::bail!("timed out loading media");
    }
    for message in doc.take_messages() {
        println!("{message}");
    }
    for (_, path, reason) in doc.missing() {
        println!("Missing media {}: {reason}", path.display());
    }
    Ok(doc)
}

fn format_time(samples: u64, rate: u32) -> String {
    let secs = samples as f64 / f64::from(rate.max(1));
    format!("{:02}:{:06.3}", (secs / 60.0) as u64, secs % 60.0)
}

pub fn play(args: &AudioArgs) -> Result<()> {
    let backend = open_backend(args)?;
    let plugins = plugin_database(&crate::prefs::Prefs::load().ui, false);
    let mut doc = load_document(&args.files, AppDirs::default(), plugins)?;
    let stream = backend.open(&args.config, &mut |info| {
        Box::new(doc.processor(info.sample_rate, info.buffer_size as usize))
    })?;
    print_stream(stream.as_ref());
    if !doc.wait_loaded(LOAD_TIMEOUT) {
        anyhow::bail!("timed out preparing media");
    }
    let rate = doc.project().sample_rate;
    let length = doc.project().length();
    if args.looping {
        doc.history_mut()
            .edit("Loop", |p| p.loop_range = Some((0, length)));
        doc.update();
    }
    doc.player_mut().engine_mut().play();
    let seconds = args.seconds.unwrap_or(if args.looping {
        LOOP_SECONDS
    } else {
        length as f64 / f64::from(rate) + 0.5
    });
    let end = Instant::now() + Duration::from_secs_f64(seconds);
    let mut stdout = std::io::stdout();
    while Instant::now() < end {
        thread::sleep(Duration::from_millis(100));
        doc.update();
        let master = doc.player().master_meter();
        let db = |ch| gain_to_db(master.take_peak(ch)).max(METER_FLOOR_DB);
        let _ = write!(
            stdout,
            "\r{} / {}   master L {:6.1} dB  R {:6.1} dB ",
            format_time(doc.player().position(), rate),
            format_time(length, rate),
            db(0),
            db(1)
        );
        let _ = stdout.flush();
    }
    println!();
    doc.player_mut().engine_mut().stop();
    print_status(stream.as_ref());
    Ok(())
}

pub fn render(args: &AudioArgs) -> Result<()> {
    let output = args.output.as_deref().context("missing --output")?;
    let rate = args.config.sample_rate.unwrap_or(RENDER_RATE);
    let plugins = plugin_database(&crate::prefs::Prefs::load().ui, false);
    let mut doc = load_document(&args.files, AppDirs::default(), plugins)?;
    let mut processor = doc.processor(rate, RENDER_BLOCK);
    if !doc.wait_loaded(LOAD_TIMEOUT) {
        anyhow::bail!("timed out preparing media");
    }
    doc.player_mut().engine_mut().play();
    let length = args.seconds.map_or_else(
        || doc.player().to_engine(doc.project().length()),
        |s| (s * f64::from(rate)) as u64,
    ) as usize;
    let mut storage = ChannelBuffers::new(0, 2, RENDER_BLOCK);
    let mut channels: Vec<Vec<f32>> = (0..2).map(|_| Vec::with_capacity(length)).collect();
    while channels[0].len() < length {
        let frames = (length - channels[0].len()).min(RENDER_BLOCK);
        processor.render(&mut storage.buffers(frames));
        for (ch, channel) in channels.iter_mut().enumerate() {
            channel.extend_from_slice(&storage.output(ch)[..frames]);
        }
    }
    let data = AudioData::new(channels, rate);
    write_wav(output, &data).with_context(|| format!("cannot write {}", output.display()))?;
    println!(
        "Rendered {:.1} s at {rate} Hz to {}",
        data.duration_secs(),
        output.display()
    );
    Ok(())
}

pub fn latency(args: &AudioArgs) -> Result<()> {
    let backend = open_backend(args)?;
    let threshold = args.threshold;
    let mut results = None;
    let stream = backend.open(&args.config, &mut |info| {
        let (mut probe, r) = LatencyProbe::new(u64::from(info.sample_rate) / 2, threshold);
        results = Some(r);
        let mut input = vec![0.0f32; 16_384];
        Box::new(move |b: &mut AudioBuffers<'_>, _: &CallbackTime| {
            let frames = b.frames().min(input.len());
            let has_input = b.input_channels() > 0;
            if has_input {
                input[..frames].copy_from_slice(&b.input(0)[..frames]);
            }
            probe.process(has_input.then_some(&input[..frames]), frames, |i, v| {
                for ch in 0..b.output_channels() {
                    b.output(ch)[i] = v;
                }
            });
        })
    })?;
    print_stream(stream.as_ref());
    println!("Sending an impulse every 0.5 s on every output, listening on input 1.");
    println!("Connect output 1 to input 1 with a cable (or hold a microphone near a speaker).");
    let results = results.context("latency probe not created")?;
    let rate = f64::from(stream.info().sample_rate);
    let ms = |frames: u64| frames as f64 * 1000.0 / rate;
    let end = Instant::now() + Duration::from_secs_f64(args.seconds.unwrap_or(DEFAULT_SECONDS));
    let mut stdout = std::io::stdout();
    while Instant::now() < end {
        thread::sleep(Duration::from_millis(250));
        if let Some(last) = results.last() {
            let _ = write!(
                stdout,
                "\rLast round trip: {last} frames ({:.2} ms), {} detections ",
                ms(last),
                results.count()
            );
            let _ = stdout.flush();
        }
    }
    println!();
    match results.min() {
        Some(min) => println!(
            "Round-trip latency: {min} frames ({:.2} ms) over {} impulses, {} missed",
            ms(min),
            results.count(),
            results.missed()
        ),
        None => println!(
            "No impulse detected. Check the loopback connection and input level, or lower --threshold."
        ),
    }
    print_status(stream.as_ref());
    Ok(())
}

pub fn compare(args: &AudioArgs) -> Result<()> {
    let [a, b] = [&args.files[0], &args.files[1]].map(|p| {
        audiostrada_media::load(p).with_context(|| format!("cannot load {}", p.display()))
    });
    let (a, b) = (a?, b?);
    if a.sample_rate() != b.sample_rate() {
        println!(
            "Sample rates differ: {} Hz vs {} Hz",
            a.sample_rate(),
            b.sample_rate()
        );
    }
    if a.channels() != b.channels() || a.frames() != b.frames() {
        println!(
            "Shapes differ: {} ch x {} frames vs {} ch x {} frames (comparing the common part)",
            a.channels(),
            a.frames(),
            b.channels(),
            b.frames()
        );
    }
    let worst = largest_difference(&a, &b);
    if worst.0 == 0.0 {
        println!("Identical (bit-exact) over the compared samples.");
    } else {
        println!(
            "Largest difference: {:.1} dBFS at channel {}, frame {} ({:.3} s)",
            gain_to_db(worst.0),
            worst.1 + 1,
            worst.2,
            worst.2 as f64 / f64::from(a.sample_rate())
        );
    }
    Ok(())
}

/// Largest absolute sample difference over the common channels and frames: (difference,
/// channel, frame).
fn largest_difference(a: &AudioData, b: &AudioData) -> (f32, usize, usize) {
    let mut worst = (0.0f32, 0usize, 0usize);
    for ch in 0..a.channels().min(b.channels()) {
        for (i, (x, y)) in a.channel(ch).iter().zip(b.channel(ch)).enumerate() {
            let diff = (x - y).abs();
            if diff > worst.0 {
                worst = (diff, ch, i);
            }
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-app-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dummy(files: Vec<PathBuf>) -> AudioArgs {
        AudioArgs {
            backend: Some(audiostrada_io::BackendKind::Dummy),
            seconds: Some(0.05),
            files,
            ..AudioArgs::default()
        }
    }

    #[test]
    fn times_are_minutes_and_seconds() {
        assert_eq!(format_time(0, 48_000), "00:00.000");
        assert_eq!(format_time(48_000 * 75 + 24_000, 48_000), "01:15.500");
        assert_eq!(
            format_time(10, 0),
            "00:10.000",
            "a zero rate does not divide by zero"
        );
    }

    #[test]
    fn meter_bar_is_clamped_to_its_width() {
        assert_eq!(meter_bar(METER_FLOOR_DB), " ".repeat(METER_WIDTH));
        assert_eq!(meter_bar(-30.0), format!("{:<40}", "#".repeat(20)));
        assert_eq!(meter_bar(6.0), "#".repeat(METER_WIDTH));
    }

    #[test]
    fn largest_difference_covers_the_common_part() {
        let a = AudioData::new(vec![vec![0.0, 0.5, 0.0], vec![0.0; 3]], 48_000);
        let b = AudioData::new(vec![vec![0.0, 0.5], vec![0.0, 0.0, -0.25]], 48_000);
        assert_eq!(largest_difference(&a, &a), (0.0, 0, 0));
        assert_eq!(largest_difference(&a, &b), (0.25, 1, 2));
        let mono = AudioData::new(vec![vec![0.0, 0.5, 0.0]], 48_000);
        assert_eq!(largest_difference(&mono, &b), (0.0, 0, 0));
    }

    #[test]
    fn projects_are_recognised_by_extension() {
        assert!(is_project(Path::new("/a/Song/Song.strada")));
        assert!(!is_project(Path::new("/a/song.wav")));
        assert!(!is_project(Path::new("audiostrada")));
    }

    #[test]
    fn render_mixes_files_into_a_wav() {
        let dir = temp_dir("render");
        let input = dir.join("tone.wav");
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        write_wav(&input, &AudioData::new(vec![samples], 48_000)).unwrap();
        let output = dir.join("mix.wav");
        let args = AudioArgs {
            output: Some(output.clone()),
            ..dummy(vec![input.clone()])
        };
        render(&args).unwrap();
        let mix = audiostrada_media::load(&output).unwrap();
        assert_eq!(mix.channels(), 2);
        assert_eq!(mix.sample_rate(), RENDER_RATE);
        assert_eq!(mix.frames(), 2400, "--seconds sets the length");
        assert!(
            mix.channel(0).iter().any(|s| s.abs() > 0.01),
            "the file is heard"
        );
        compare(&dummy(vec![input.clone(), output])).unwrap();
        compare(&dummy(vec![input.clone(), input])).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_files_are_errors() {
        let missing = PathBuf::from("/nonexistent/audiostrada/a.wav");
        assert!(compare(&dummy(vec![missing.clone(), missing.clone()])).is_err());
        let project = PathBuf::from("/nonexistent/audiostrada/Song.strada");
        assert!(load_document(&[project], AppDirs::default(), Database::default()).is_err());
    }

    #[test]
    fn device_commands_run_on_the_dummy_backend() {
        let args = dummy(Vec::new());
        devices(&args).unwrap();
        tone(&args).unwrap();
        monitor(&args).unwrap();
        latency(&args).unwrap();
    }

    #[test]
    fn play_runs_on_the_dummy_backend() {
        let dir = temp_dir("play");
        let input = dir.join("tone.wav");
        write_wav(&input, &AudioData::new(vec![vec![0.25; 4800]], 48_000)).unwrap();
        let args = AudioArgs {
            looping: true,
            ..dummy(vec![input])
        };
        play(&args).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
