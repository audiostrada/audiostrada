mod audio;
mod cli;
mod license;
mod logging;
mod plugins;
mod prefs;

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use audiostrada_ui::document::{AppDirs, Document};
use cli::Command;

audiostrada_rt::install_alloc_checker!();

fn main() -> ExitCode {
    logging::init();
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(e) => {
            eprintln!("error: {e}\n\n{}", cli::USAGE);
            return ExitCode::FAILURE;
        }
    };
    let result = match command {
        Command::Run(run) => run_ui(&run),
        Command::Help => {
            println!("{}", cli::USAGE);
            Ok(())
        }
        Command::Diagnose => {
            audio::diagnose();
            Ok(())
        }
        Command::Devices(args) => audio::devices(&args),
        Command::Tone(args) => audio::tone(&args),
        Command::Monitor(args) => audio::monitor(&args),
        Command::Play(args) => audio::play(&args),
        Command::Render(args) => audio::render(&args),
        Command::Latency(args) => audio::latency(&args),
        Command::Compare(args) => audio::compare(&args),
        Command::Plugins { rescan } => plugins::list(rescan),
        Command::PluginScan(format, path) => {
            audiostrada_plugins::quiet_failures();
            plugins::scan_one(&format, &path)
        }
        Command::PluginHost => {
            audiostrada_plugins::quiet_failures();
            audiostrada_plugins::sandbox::run_host();
            Ok(())
        }
        Command::Mcp { connect: None } => {
            // The plugins installed, as in the window and `render`.
            let mut backend = audiostrada_ui::control::Headless::with_plugins(
                audio::plugin_database(&prefs::Prefs::load().ui, false),
            );
            let stdin = std::io::stdin();
            audiostrada_ui::control::serve(&mut backend, stdin.lock(), std::io::stdout())
                .map_err(Into::into)
        }
        Command::Mcp {
            connect: Some(port),
        } => audiostrada_ui::control::serve_remote(port).map_err(anyhow::Error::msg),
        Command::License(cli::LicenseCommand::Keygen) => license::keygen(),
        Command::License(cli::LicenseCommand::Check { file, key }) => license::check(&file, &key),
        Command::License(cli::LicenseCommand::Issue {
            secret_file,
            name,
            email,
            until,
            commercial,
        }) => license::issue(&secret_file, &name, &email, &until, commercial),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn app_dirs() -> AppDirs {
    AppDirs {
        recovery: dirs::data_dir().map(|d| d.join("audiostrada").join("recovery")),
        peaks: dirs::cache_dir().map(|d| d.join("audiostrada").join("peaks")),
        recordings: dirs::data_dir().map(|d| d.join("audiostrada").join("recordings")),
    }
}

/// Closes the console Windows opens for a console program started from the Start menu or
/// Explorer; a console shared with a shell (command line use) stays.
#[cfg(windows)]
fn detach_own_console() {
    let mut processes = [0u32; 2];
    // SAFETY: plain calls with a valid buffer.
    unsafe {
        if windows::Win32::System::Console::GetConsoleProcessList(&mut processes) == 1 {
            let _ = windows::Win32::System::Console::FreeConsole();
        }
    }
}

fn run_ui(run: &cli::RunArgs) -> anyhow::Result<()> {
    let files = &run.files;
    #[cfg(windows)]
    detach_own_console();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "AudioStrada starting");
    let prefs = Rc::new(RefCell::new(prefs::Prefs::load()));
    let dirs = app_dirs();
    let project_file = files.iter().find(|f| audio::is_project(f));
    let recovery = audiostrada_ui::RecoveryOffer::find(project_file.map(|p| p.as_path()), &dirs);
    // Before the project, so that its plugins load with it.
    let database = audio::plugin_database(&prefs.borrow().ui, run.disable_plugins);
    let doc = if recovery.is_some() {
        let mut doc = Document::new(dirs.clone());
        doc.set_plugins(database);
        doc
    } else {
        audio::load_document(files, dirs.clone(), database)?
    };
    let doc = Rc::new(RefCell::new(doc));
    let mut host = audiostrada_ui::AudioHost::new({
        let doc = Rc::clone(&doc);
        Box::new(move |info| {
            Box::new(
                doc.borrow_mut()
                    .processor(info.sample_rate, info.buffer_size as usize),
            )
        })
    });
    let (backend, config, mut recent, ui_prefs) = {
        let prefs = prefs.borrow();
        (
            prefs.audio.backend(),
            prefs.audio.config(),
            prefs.recent.clone(),
            prefs.ui.clone(),
        )
    };
    host.start(backend, config);
    if let Some(paths) = doc.borrow().paths() {
        recent.retain(|p| *p != paths.file);
        recent.insert(0, paths.file.clone());
    }
    let hooks = audiostrada_ui::Hooks {
        audio_changed: {
            let prefs = Rc::clone(&prefs);
            Box::new(move |host| {
                let mut prefs = prefs.borrow_mut();
                prefs.audio = prefs::AudioPrefs::from_config(host.kind(), host.config());
                prefs.save();
            })
        },
        recent_changed: {
            let prefs = Rc::clone(&prefs);
            Box::new(move |recent| {
                let mut prefs = prefs.borrow_mut();
                prefs.recent = recent.to_vec();
                prefs.save();
            })
        },
        ui_changed: {
            let prefs = Rc::clone(&prefs);
            Box::new(move |ui| {
                let mut prefs = prefs.borrow_mut();
                prefs.ui = ui.clone();
                prefs.save();
            })
        },
        plugin_scan: Box::new(|extra, full, progress| {
            let cache = plugins::database_path()
                .unwrap_or_else(|| std::env::temp_dir().join("audiostrada-plugins.json"));
            let exe = audiostrada_plugins::own_executable().unwrap_or_else(|| "audiostrada".into());
            audiostrada_plugins::scan::scan_in_background(cache, extra, exe, full, progress)
        }),
    };

    let mut app =
        audiostrada_ui::AudioStradaApp::new(host, doc, dirs, ui_prefs, recent, recovery, hooks);
    if let Some(port) = run.control {
        app.listen(port).map_err(anyhow::Error::msg)?;
    }
    audiostrada_ui::run(app).map_err(|e| anyhow::anyhow!("{e}"))
}
