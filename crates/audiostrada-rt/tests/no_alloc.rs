use std::thread;

use audiostrada_rt::{alloc_violations, channel, permit_alloc, rt_log, rt_section};

audiostrada_rt::install_alloc_checker!();

#[cfg(debug_assertions)]
#[test]
fn allocation_in_rt_section_is_detected() {
    let before = alloc_violations();
    rt_section(|| drop(std::hint::black_box(vec![0u8; 16])));
    assert!(alloc_violations() > before);
}

#[test]
fn permitted_allocation_is_not_a_violation() {
    let before = alloc_violations();
    rt_section(|| permit_alloc(|| drop(std::hint::black_box(vec![0u8; 16]))));
    assert_eq!(alloc_violations(), before);
}

#[test]
fn channel_and_logger_are_allocation_free() {
    let (mut tx, mut rx) = channel::<u64>(16);
    let (mut logger, drain) = rt_log(16);
    let before = alloc_violations();
    rt_section(|| {
        for i in 0..32 {
            let _ = tx.try_send(i);
        }
        while rx.try_recv().is_some() {}
        logger.warn("xrun", &[64u32.into(), "input".into(), 0.5f64.into()]);
        for _ in 0..32 {
            logger.info("spam", &[]);
        }
    });
    assert_eq!(alloc_violations(), before);
    assert!(drain.dropped() > 0);
}

#[test]
fn events_cross_threads() {
    let (mut logger, mut drain) = rt_log(64);
    let rt = thread::spawn(move || {
        let before = alloc_violations();
        rt_section(|| {
            for i in 0..10u32 {
                logger.debug("block", &[i.into()]);
            }
        });
        alloc_violations() - before
    });
    assert_eq!(rt.join().unwrap(), 0);
    let mut values = Vec::new();
    drain.drain_with(|e| values.push(e.args()[0]));
    assert_eq!(values.len(), 10);
}
