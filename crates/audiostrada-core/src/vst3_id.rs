//! VST3 class ids (32 hex digits). Other hosts and files (Reaper, DAWproject, `.vstpreset`)
//! write Steinberg's canonical form; AudioStrada keeps the plugin's bytes as they are, and on
//! Windows those hold their first three fields (4, 2 and 2 bytes) little-endian, COM style.

/// The id with its first three fields byte-reversed: the Windows bytes from the canonical
/// form and back. `None` when it is not 32 hex digits.
pub fn swapped(id: &str) -> Option<String> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| &id[i * 2..i * 2 + 2];
    let order = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    Some(
        order
            .iter()
            .map(|&i| byte(i).to_ascii_uppercase())
            .collect(),
    )
}

/// AudioStrada's id on this system from a canonical one.
pub fn from_canonical(id: &str) -> String {
    let id = id.to_ascii_uppercase();
    if cfg!(windows) {
        swapped(&id).unwrap_or(id)
    } else {
        id
    }
}

/// The canonical id from AudioStrada's on this system.
pub fn to_canonical(id: &str) -> String {
    // The swap undoes itself.
    from_canonical(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaper_and_windows_ids_swap_into_each_other() {
        // SSL Native X-EQ 2 as Reaper writes it, then as its Windows bytes read.
        let canonical = "565354534E455173736C206E61746976";
        let windows = "53545356454E7351736C206E61746976";
        assert_eq!(swapped(canonical).as_deref(), Some(windows));
        assert_eq!(swapped(windows).as_deref(), Some(canonical));
        assert_eq!(swapped("abc"), None);
        let local = from_canonical(&canonical.to_ascii_lowercase());
        assert_eq!(local, if cfg!(windows) { windows } else { canonical });
        assert_eq!(to_canonical(&local), canonical);
    }
}
