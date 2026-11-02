//! MIDI input devices (ALSA, CoreMIDI, WinMM through midir), feeding the live ring.

use midir::{MidiInput, MidiInputConnection};

use crate::live::LiveMidi;

const CLIENT: &str = "AudioStrada";

/// Names of the MIDI inputs present now.
pub fn input_names() -> Vec<String> {
    let Ok(input) = MidiInput::new(CLIENT) else {
        return Vec::new();
    };
    input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .filter(|n| !own_port(n))
        .collect()
}

/// Ports AudioStrada itself creates (ALSA lists them as "AudioStrada:..."), never offered as devices.
fn own_port(name: &str) -> bool {
    name.strip_prefix(CLIENT)
        .is_some_and(|rest| rest.starts_with(':'))
}

/// Open input connections.
pub struct MidiInputs {
    connections: Vec<(String, MidiInputConnection<()>)>,
}

impl std::fmt::Debug for MidiInputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.connections.iter().map(|c| &c.0))
            .finish()
    }
}

impl MidiInputs {
    /// Opens every input `wanted` accepts; messages are published with the engine position
    /// given by `position`. Returns the inputs and the errors met.
    pub fn open(
        wanted: impl Fn(&str) -> bool,
        live: &LiveMidi,
        position: impl Fn() -> u64 + Send + Sync + Clone + 'static,
    ) -> (Self, Vec<String>) {
        let mut connections = Vec::new();
        let mut errors = Vec::new();
        let count = MidiInput::new(CLIENT).map_or(0, |i| i.port_count());
        for index in 0..count {
            let Ok(input) = MidiInput::new(CLIENT) else {
                continue;
            };
            let Some(port) = input.ports().into_iter().nth(index) else {
                continue;
            };
            let Ok(name) = input.port_name(&port) else {
                continue;
            };
            if own_port(&name) || !wanted(&name) {
                continue;
            }
            let live = live.clone();
            let position = position.clone();
            let callback = move |_stamp: u64, message: &[u8], _: &mut ()| {
                if let Some(data) = channel_message(message) {
                    live.push(data, position());
                }
            };
            match input.connect(&port, "AudioStrada input", callback, ()) {
                Ok(connection) => connections.push((name, connection)),
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        (Self { connections }, errors)
    }

    pub fn names(&self) -> Vec<String> {
        self.connections.iter().map(|c| c.0.clone()).collect()
    }
}

/// Names of the MIDI outputs present now.
pub fn output_names() -> Vec<String> {
    let Ok(output) = midir::MidiOutput::new(CLIENT) else {
        return Vec::new();
    };
    output
        .ports()
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .filter(|n| !own_port(n))
        .collect()
}

/// Rings to forward and the output port each one goes to.
pub type Routes = Vec<(String, crate::live::LiveReader)>;

/// Forwards MIDI from the engine to output ports on its own thread, opening ports as needed.
pub struct MidiOutputs {
    routes: std::sync::Arc<std::sync::Mutex<(Routes, bool)>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for MidiOutputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidiOutputs").finish_non_exhaustive()
    }
}

impl MidiOutputs {
    pub fn start() -> Self {
        use std::sync::atomic::Ordering;
        let routes: std::sync::Arc<std::sync::Mutex<(Routes, bool)>> = Default::default();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (r, s) = (std::sync::Arc::clone(&routes), std::sync::Arc::clone(&stop));
        let thread = std::thread::Builder::new()
            .name("midi-out".into())
            .spawn(move || {
                let mut open: Vec<(String, midir::MidiOutputConnection)> = Vec::new();
                let mut failed: Vec<String> = Vec::new();
                while !s.load(Ordering::Relaxed) {
                    let mut pending: Vec<(String, [u8; 3])> = Vec::new();
                    if let Ok(mut guard) = r.lock() {
                        let (routes, changed) = &mut *guard;
                        if std::mem::take(changed) {
                            failed.clear();
                            open.retain(|o| routes.iter().any(|r| r.0 == o.0));
                        }
                        for (port, reader) in routes.iter_mut() {
                            reader.drain(|data, _| pending.push((port.clone(), data)));
                        }
                    }
                    for (port, data) in pending {
                        if !open.iter().any(|o| o.0 == port) && !failed.contains(&port) {
                            match connect(&port) {
                                Some(c) => open.push((port.clone(), c)),
                                None => failed.push(port.clone()),
                            }
                        }
                        if let Some((_, c)) = open.iter_mut().find(|o| o.0 == port) {
                            let _ = c.send(&data[..message_len(data[0])]);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            })
            .ok();
        Self {
            routes,
            stop,
            thread,
        }
    }

    /// Replaces what is forwarded; ports that failed to open are retried.
    pub fn set_routes(&self, routes: Routes) {
        if let Ok(mut r) = self.routes.lock() {
            *r = (routes, true);
        }
    }
}

fn connect(port: &str) -> Option<midir::MidiOutputConnection> {
    let output = midir::MidiOutput::new(CLIENT).ok()?;
    let found = output
        .ports()
        .into_iter()
        .find(|p| output.port_name(p).ok().as_deref() == Some(port))?;
    output.connect(&found, "AudioStrada output").ok()
}

impl Drop for MidiOutputs {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A channel message from a device as three bytes; system messages are ignored.
fn channel_message(message: &[u8]) -> Option<[u8; 3]> {
    let &status = message.first()?;
    (0x80..0xF0).contains(&status).then(|| {
        [
            status,
            message.get(1).copied().unwrap_or(0),
            message.get(2).copied().unwrap_or(0),
        ]
    })
}

/// Bytes of a channel message: program change and channel pressure have one data byte.
fn message_len(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_ports_are_hidden() {
        assert!(own_port("AudioStrada:AudioStrada input 128:0"));
        assert!(!own_port("AudioStrada"));
        assert!(!own_port("AudioStradas:in"));
        assert!(!own_port("Midi Through:Port-0"));
    }

    #[test]
    fn only_channel_messages_are_kept() {
        assert_eq!(channel_message(&[0x91, 60, 100]), Some([0x91, 60, 100]));
        assert_eq!(channel_message(&[0xC0, 5]), Some([0xC0, 5, 0]));
        assert_eq!(channel_message(&[0xF8]), None, "clock");
        assert_eq!(channel_message(&[0xF0, 1, 2, 0xF7]), None, "sysex");
        assert_eq!(channel_message(&[0x40, 1]), None, "running status data");
        assert_eq!(channel_message(&[]), None);
    }

    #[test]
    fn message_lengths() {
        assert_eq!(message_len(0x93), 3);
        assert_eq!(message_len(0xB0), 3);
        assert_eq!(message_len(0xC5), 2);
        assert_eq!(message_len(0xDF), 2);
        assert_eq!(message_len(0xE0), 3);
    }
}
