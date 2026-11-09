//! Real-time scheduling for audio threads created by AudioStrada itself.

/// Priority used for audio threads, below the usual JACK/PipeWire driver priority.
pub const AUDIO_PRIORITY: i32 = 70;

/// Turns real-time scheduling off for this process and the plugin hosts it starts.
pub const NO_REALTIME_ENV: &str = "AUDIOSTRADA_NO_REALTIME";

static FORBIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keeps every thread of this process at the normal priority, as tests do: real-time threads
/// busy rendering take the processor from the whole desktop.
pub fn forbid_realtime() {
    FORBIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether threads may be promoted: not after [`forbid_realtime`] nor with
/// [`NO_REALTIME_ENV`] set.
pub fn realtime_allowed() -> bool {
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !FORBIDDEN.load(std::sync::atomic::Ordering::Relaxed)
        && !*ENV.get_or_init(|| std::env::var_os(NO_REALTIME_ENV).is_some_and(|v| v != "0"))
}

fn refused() -> Result<(), String> {
    if realtime_allowed() {
        Ok(())
    } else {
        Err("real-time scheduling is off".into())
    }
}

/// Switches the current thread to real-time scheduling.
#[cfg(target_os = "linux")]
pub fn promote_current_thread(priority: i32) -> Result<(), String> {
    refused()?;
    // SAFETY: plain libc calls on the current thread with an initialized sched_param.
    unsafe {
        let mut param: libc::sched_param = std::mem::zeroed();
        param.sched_priority = priority;
        match libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param) {
            0 => Ok(()),
            err => Err(std::io::Error::from_raw_os_error(err).to_string()),
        }
    }
}

/// Registers the current thread with MMCSS ("Pro Audio" task); the priority is chosen by Windows.
#[cfg(windows)]
pub fn promote_current_thread(_priority: i32) -> Result<(), String> {
    refused()?;
    let mut task_index = 0u32;
    // SAFETY: the task name is a static wide string and the index pointer is valid.
    unsafe {
        windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW(
            windows::core::w!("Pro Audio"),
            &mut task_index,
        )
    }
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn invalid_priority_is_an_error() {
        let result = std::thread::spawn(|| super::promote_current_thread(1000))
            .join()
            .unwrap();
        assert!(result.is_err());
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn promote_current_thread(_priority: i32) -> Result<(), String> {
    Err("real-time promotion is not implemented on this platform".into())
}

/// Makes the current thread a real-time audio thread that runs once per `period` (a device
/// block): time-constraint scheduling on macOS, [`AUDIO_PRIORITY`] elsewhere.
#[cfg(not(target_os = "macos"))]
pub fn promote_audio_thread(_period: std::time::Duration) -> Result<(), String> {
    promote_current_thread(AUDIO_PRIORITY)
}

/// Makes the current thread a real-time audio thread that runs once per `period` (a device
/// block): time-constraint scheduling, like Core Audio's own I/O thread, so that the system's
/// load (indexing, other applications) does not delay it.
#[cfg(target_os = "macos")]
#[allow(
    deprecated,
    reason = "libc's mach timebase rather than the mach2 crate for one call"
)]
pub fn promote_audio_thread(period: std::time::Duration) -> Result<(), String> {
    use std::time::Duration;
    refused()?;
    let mut timebase = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: fills a valid struct.
    unsafe { libc::mach_timebase_info(&mut timebase) };
    if timebase.numer == 0 {
        return Err("no mach timebase".into());
    }
    let ticks = |d: Duration| {
        (d.as_nanos() * u128::from(timebase.denom) / u128::from(timebase.numer))
            .min(u32::MAX as u128) as u32
    };
    // The kernel takes 50 µs to 50 ms of computation per period.
    let period = period.clamp(Duration::from_micros(500), Duration::from_millis(100));
    let computation = (period / 2).clamp(Duration::from_micros(50), Duration::from_millis(50));
    let mut policy = libc::thread_time_constraint_policy {
        period: ticks(period),
        computation: ticks(computation),
        constraint: ticks(period),
        preemptible: 1,
    };
    // SAFETY: the current thread's port and a policy of the flavor and count given.
    let result = unsafe {
        libc::thread_policy_set(
            libc::pthread_mach_thread_np(libc::pthread_self()),
            libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
            (&raw mut policy).cast(),
            libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    match result {
        0 => Ok(()),
        err => Err(format!("thread_policy_set failed ({err})")),
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    #[test]
    fn audio_threads_get_time_constraints() {
        if !super::realtime_allowed() {
            return;
        }
        let result = std::thread::spawn(|| {
            super::promote_audio_thread(std::time::Duration::from_micros(2_667))
        })
        .join()
        .unwrap();
        assert_eq!(result, Ok(()));
    }
}
