//! VST3 states between AudioStrada's form (`SV3\0`, the component state's length, the
//! component then the controller state) and Steinberg's `.vstpreset` files.

/// A `.vstpreset` holding the component and controller states of the plugin `class` (32 hex
/// digits).
pub fn to_preset(class: &str, state: &[u8]) -> Option<Vec<u8>> {
    let rest = state.strip_prefix(b"SV3\0")?;
    let (len, rest) = rest.split_first_chunk::<4>()?;
    let len = u32::from_le_bytes(*len) as usize;
    if len > rest.len() || class.len() != 32 {
        return None;
    }
    let (component, controller) = rest.split_at(len);
    let mut out = b"VST3".to_vec();
    out.extend_from_slice(&1i32.to_le_bytes());
    out.extend_from_slice(class.to_ascii_uppercase().as_bytes());
    let list = 48 + component.len() + controller.len();
    out.extend_from_slice(&(list as i64).to_le_bytes());
    out.extend_from_slice(component);
    out.extend_from_slice(controller);
    let mut chunks = vec![(b"Comp", 48, component.len())];
    if !controller.is_empty() {
        chunks.push((b"Cont", 48 + component.len(), controller.len()));
    }
    out.extend_from_slice(b"List");
    out.extend_from_slice(&(chunks.len() as i32).to_le_bytes());
    for (id, offset, size) in chunks {
        out.extend_from_slice(id);
        out.extend_from_slice(&(offset as i64).to_le_bytes());
        out.extend_from_slice(&(size as i64).to_le_bytes());
    }
    Some(out)
}

/// AudioStrada's state from a `.vstpreset`.
pub fn from_preset(preset: &[u8]) -> Option<Vec<u8>> {
    if preset.get(..4)? != b"VST3" {
        return None;
    }
    let list = usize::try_from(i64::from_le_bytes(preset.get(40..48)?.try_into().ok()?)).ok()?;
    if preset.get(list..list + 4)? != b"List" {
        return None;
    }
    let count = i32::from_le_bytes(preset.get(list + 4..list + 8)?.try_into().ok()?);
    let (mut component, mut controller): (&[u8], &[u8]) = (&[], &[]);
    for i in 0..usize::try_from(count).ok()? {
        let at = list + 8 + i * 20;
        let id = preset.get(at..at + 4)?;
        let offset = usize::try_from(i64::from_le_bytes(
            preset.get(at + 4..at + 12)?.try_into().ok()?,
        ))
        .ok()?;
        let size = usize::try_from(i64::from_le_bytes(
            preset.get(at + 12..at + 20)?.try_into().ok()?,
        ))
        .ok()?;
        let data = preset.get(offset..offset + size)?;
        match id {
            b"Comp" => component = data,
            b"Cont" => controller = data,
            _ => {}
        }
    }
    let mut out = b"SV3\0".to_vec();
    out.extend_from_slice(&(component.len() as u32).to_le_bytes());
    out.extend_from_slice(component);
    out.extend_from_slice(controller);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vst3_state_goes_through_a_preset_file_unchanged() {
        let mut state = b"SV3\0".to_vec();
        state.extend_from_slice(&3u32.to_le_bytes());
        state.extend_from_slice(b"abcde");
        let class = "0123456789abcdef0123456789ABCDEF";
        let preset = to_preset(class, &state).unwrap();
        assert_eq!(&preset[8..40], class.to_ascii_uppercase().as_bytes());
        assert_eq!(from_preset(&preset).unwrap(), state);
        assert!(from_preset(b"nope").is_none());
    }
}
