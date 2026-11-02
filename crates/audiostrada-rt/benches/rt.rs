use std::hint::black_box;

use audiostrada_rt::{channel, rt_log};
use criterion::{Criterion, criterion_group, criterion_main};

fn channel_roundtrip(c: &mut Criterion) {
    let (mut tx, mut rx) = channel::<[f32; 4]>(1024);
    c.bench_function("channel_roundtrip", |b| {
        b.iter(|| {
            let _ = tx.try_send(black_box([0.5; 4]));
            black_box(rx.try_recv())
        })
    });
}

fn rt_log_event(c: &mut Criterion) {
    let (mut logger, mut drain) = rt_log(1024);
    c.bench_function("rt_log_event", |b| {
        b.iter(|| {
            logger.warn(
                "xrun",
                &[black_box(256u32).into(), black_box(0.5f32).into()],
            );
            drain.drain_with(|e| {
                black_box(e);
            })
        })
    });
}

criterion_group!(benches, channel_roundtrip, rt_log_event);
criterion_main!(benches);
