//! Time stamps as typed into transcripts, and parsing of user-entered times.

/// Formats `ms` as `hh:mm:ss` (with `.t` tenths when `tenths` is true), e.g. `00:01:23.4`.
pub fn format_timestamp(ms: u64, tenths: bool) -> String {
    let h = ms / 3_600_000;
    let m = ms / 60_000 % 60;
    let s = ms / 1_000 % 60;
    if tenths { format!("{h:02}:{m:02}:{s:02}.{}", ms / 100 % 10) } else { format!("{h:02}:{m:02}:{s:02}") }
}

/// Parses `ss`, `mm:ss`, `hh:mm:ss`, each optionally with a decimal fraction of a second
/// (`1:20.5`), or a plain number of milliseconds with an `ms` suffix (`1500ms`).
/// Returns `None` for anything else, including values that overflow.
pub fn parse_time(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Some(ms) = text.strip_suffix("ms") {
        return ms.trim().parse().ok();
    }
    let parts: Vec<&str> = text.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let (whole, last) = parts.split_at(parts.len() - 1);
    let last = last.first()?;
    let (secs, frac) = match last.split_once('.') {
        Some((s, f)) => (s, f),
        None => (*last, ""),
    };
    if frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut ms: u64 = 0;
    for (i, p) in whole.iter().enumerate() {
        let v: u64 = p.parse().ok()?;
        // Hours are unbounded; minutes after hours must be < 60.
        if i > 0 && v >= 60 {
            return None;
        }
        ms = ms.checked_mul(60)?.checked_add(v)?;
    }
    let secs: u64 = secs.parse().ok()?;
    if !whole.is_empty() && secs >= 60 {
        return None;
    }
    let frac_ms: u64 = if frac.is_empty() { 0 } else { format!("{frac:0<3}").parse().ok()? };
    ms.checked_mul(60)?.checked_add(secs)?.checked_mul(1000)?.checked_add(frac_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_parses() {
        assert_eq!(format_timestamp(83_456, true), "00:01:23.4");
        assert_eq!(format_timestamp(3_723_000, false), "01:02:03");
        assert_eq!(parse_time("1:20"), Some(80_000));
        assert_eq!(parse_time("01:02:03.5"), Some(3_723_500));
        assert_eq!(parse_time("90"), Some(90_000));
        assert_eq!(parse_time("1500ms"), Some(1_500));
        assert_eq!(parse_time("0.25"), Some(250));
        for bad in ["", "a", "1:60", "1:2:3:4", "1.2.3", "-5", "99999999999999999999", "1:00.1234", "1:x"] {
            assert_eq!(parse_time(bad), None, "{bad:?}");
        }
    }

    proptest::proptest! {
        #[test]
        fn parse_is_total_and_round_trips(ms in 0u64..400_000_000_000) {
            let tenth = ms / 100 * 100;
            proptest::prop_assert_eq!(parse_time(&format_timestamp(tenth, true)), Some(tenth));
        }

        #[test]
        fn never_panics(s in "\\PC{0,20}") {
            let _ = parse_time(&s);
        }
    }
}
