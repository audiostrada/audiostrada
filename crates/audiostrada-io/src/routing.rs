/// Pairs device channels with stream channels one to one; a mono device is linked to every
/// channel (fed by all of them on output, which sums them).
pub(crate) fn pair_ports<T: Clone>(device: &[T], ours: &[T]) -> Vec<(T, T)> {
    match device {
        [] => Vec::new(),
        [mono] => ours.iter().map(|o| (mono.clone(), o.clone())).collect(),
        _ => device.iter().cloned().zip(ours.iter().cloned()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_device_feeds_all_ports() {
        assert_eq!(pair_ports(&[1], &[10, 11]), vec![(1, 10), (1, 11)]);
        assert_eq!(pair_ports(&[1, 2, 3], &[10, 11]), vec![(1, 10), (2, 11)]);
        assert_eq!(pair_ports(&[1, 2], &[10, 11, 12]), vec![(1, 10), (2, 11)]);
        assert!(pair_ports::<u32>(&[], &[10]).is_empty());
    }
}
