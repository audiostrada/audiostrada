//! Real-time scheduling for audio threads created by AudioStrada itself.

/// Priority used for audio threads, below the usual JACK/PipeWire driver priority.
pub const AUDIO_PRIORITY: i32 = 70;

/// Switches the current thread to real-time scheduling.
#[cfg(target_os = "linux")]
pub fn promote_current_thread(priority: i32) -> Result<(), String> {
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
