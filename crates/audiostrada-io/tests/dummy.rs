use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use audiostrada_io::{
    AudioBuffers, BackendKind, CallbackTime, IoError, StreamConfig, backend, open_stream,
};

audiostrada_rt::install_alloc_checker!();

fn wait_for(mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < Duration::from_secs(5), "timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn callback_receives_requested_layout_and_position() {
    let config = StreamConfig {
        sample_rate: Some(48_000),
        buffer_size: 64,
        inputs: 1,
        outputs: 3,
        ..StreamConfig::default()
    };
    let last_position = Arc::new(AtomicU64::new(0));
    let layout = Arc::new(AtomicUsize::new(0));
    let callback = {
        let last_position = Arc::clone(&last_position);
        let layout = Arc::clone(&layout);
        move |buffers: &mut AudioBuffers<'_>, time: &CallbackTime| {
            layout.store(
                buffers.frames() * 100 + buffers.input_channels() * 10 + buffers.output_channels(),
                Ordering::Relaxed,
            );
            last_position.store(time.position, Ordering::Relaxed);
            buffers.output(2).fill(0.5);
        }
    };
    let backend = backend(BackendKind::Dummy).unwrap();
    let stream = open_stream(backend.as_ref(), &config, Box::new(callback)).unwrap();
    wait_for(|| stream.status().cycles() >= 4);
    assert_eq!(layout.load(Ordering::Relaxed), 6413);
    assert_eq!(last_position.load(Ordering::Relaxed) % 64, 0);
    assert_eq!(stream.info().buffer_latency_ms(), 64.0 * 1000.0 / 48_000.0);
    assert_eq!(stream.status().alloc_violations(), 0);
}

#[test]
fn panicking_callback_is_isolated() {
    let backend = backend(BackendKind::Dummy).unwrap();
    let stream = open_stream(
        backend.as_ref(),
        &StreamConfig::default(),
        Box::new(|_: &mut AudioBuffers<'_>, _: &CallbackTime| panic!("boom")),
    )
    .unwrap();
    wait_for(|| stream.status().panicked() && stream.status().cycles() >= 3);
}

#[cfg(debug_assertions)]
#[test]
fn allocations_in_callback_are_reported() {
    let backend = backend(BackendKind::Dummy).unwrap();
    let stream = open_stream(
        backend.as_ref(),
        &StreamConfig::default(),
        Box::new(|_: &mut AudioBuffers<'_>, _: &CallbackTime| {
            drop(std::hint::black_box(vec![0u8; 8]));
        }),
    )
    .unwrap();
    wait_for(|| stream.status().alloc_violations() > 0);
}

#[test]
fn unknown_device_is_rejected() {
    let config = StreamConfig {
        output_device: Some("nope".into()),
        ..StreamConfig::default()
    };
    let backend = backend(BackendKind::Dummy).unwrap();
    let result = open_stream(
        backend.as_ref(),
        &config,
        Box::new(|_: &mut AudioBuffers<'_>, _: &CallbackTime| {}),
    );
    assert!(matches!(result, Err(IoError::DeviceNotFound(_))));
}

#[test]
fn factory_receives_the_stream_format() {
    let config = StreamConfig {
        sample_rate: Some(44_100),
        buffer_size: 128,
        ..StreamConfig::default()
    };
    let mut seen = None;
    let stream = backend(BackendKind::Dummy)
        .unwrap()
        .open(&config, &mut |info| {
            seen = Some((info.sample_rate, info.buffer_size));
            Box::new(|_: &mut AudioBuffers<'_>, _: &CallbackTime| {})
        })
        .unwrap();
    assert_eq!(seen, Some((44_100, 128)));
    assert_eq!(stream.info().sample_rate, 44_100);
}

#[test]
fn invalid_formats_are_rejected() {
    let backend = backend(BackendKind::Dummy).unwrap();
    let configs = [
        StreamConfig {
            sample_rate: Some(0),
            ..StreamConfig::default()
        },
        StreamConfig {
            buffer_size: 0,
            ..StreamConfig::default()
        },
        StreamConfig {
            outputs: 65,
            ..StreamConfig::default()
        },
    ];
    for config in configs {
        let result = open_stream(
            backend.as_ref(),
            &config,
            Box::new(|_: &mut AudioBuffers<'_>, _: &CallbackTime| {}),
        );
        assert!(
            matches!(result, Err(IoError::UnsupportedConfig(_))),
            "{config:?}"
        );
    }
}

#[test]
fn dummy_device_is_listed_and_selectable() {
    let backend = backend(BackendKind::Dummy).unwrap();
    assert_eq!(backend.kind(), BackendKind::Dummy);
    assert_eq!(backend.device_generation(), 0);
    let devices = backend.devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert!(devices[0].is_default_input && devices[0].is_default_output);
    let config = StreamConfig {
        input_device: Some(devices[0].id.clone()),
        output_device: Some(devices[0].id.clone()),
        ..StreamConfig::default()
    };
    let stream = open_stream(
        backend.as_ref(),
        &config,
        Box::new(|_: &mut AudioBuffers<'_>, _: &CallbackTime| {}),
    )
    .unwrap();
    assert_eq!(stream.info().sample_rate, 48_000);
    wait_for(|| stream.status().current_buffer_size() == 256);
    assert_eq!(stream.status().current_sample_rate(), 48_000);
    assert!(!stream.status().failed());
}

#[test]
fn backends_of_other_systems_are_unavailable() {
    let compiled = audiostrada_io::compiled_backends();
    assert_eq!(compiled.last(), Some(&BackendKind::Dummy));
    for kind in BackendKind::ALL {
        if !compiled.contains(&kind) {
            assert!(matches!(
                backend(kind),
                Err(IoError::BackendUnavailable(k)) if k == kind
            ));
        }
    }
}
