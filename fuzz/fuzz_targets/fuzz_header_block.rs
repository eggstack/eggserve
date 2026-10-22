#![no_main]
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderError, HeaderName, HeaderValue};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Split input: first section for name, second for value, third for block ops
    if data.len() < 4 {
        return;
    }
    let name_end = (data[0] as usize).min(data.len() - 1).max(1);
    let value_end = (data[1] as usize % data.len())
        .max(name_end + 1)
        .min(data.len());
    let block_byte = data[2];
    let lookup_byte = data[3];

    let name_bytes = &data[1..name_end];
    let value_bytes = &data[name_end..value_end];

    // Fuzz HeaderName
    if let Ok(name_str) = std::str::from_utf8(name_bytes) {
        let result = HeaderName::new(name_str);
        match result {
            Ok(name) => {
                assert_eq!(name.as_str(), name_str);
                assert!(!name.as_str().is_empty());
                assert!(name.as_str().len() <= 256);
                assert_eq!(format!("{}", name), name_str);
            }
            Err(e) => {
                assert!(
                    matches!(e, HeaderError::InvalidName | HeaderError::NameTooLong),
                    "unexpected error variant: {:?}",
                    e
                );
            }
        }
    }

    // Fuzz HeaderValue
    if let Ok(value_str) = std::str::from_utf8(value_bytes) {
        let result = HeaderValue::new(value_str);
        match result {
            Ok(value) => {
                // The stored octets are the input with the surrounding
                // optional whitespace (SP/HTAB) removed, per the documented
                // `OWS` invariant on `HeaderValue`. `HeaderValue` exposes the
                // stored text through `to_str()` (which fails only for
                // non-UTF-8 octets — impossible for a UTF-8 input).
                let expected = value_str.trim_matches(|c: char| c == ' ' || c == '\t');
                assert_eq!(
                    value
                        .to_str()
                        .expect("value constructed from UTF-8 input is valid UTF-8"),
                    expected
                );
                assert_eq!(format!("{}", value), expected);
            }
            Err(e) => {
                assert_eq!(e, HeaderError::InvalidValue);
                // The rejected value must contain a byte outside the accepted
                // domain: only HTAB, SP–`~` and obs-text are accepted, so any
                // other CTL (CR, LF, NUL, …) and DEL are refused.
                assert!(
                    value_str
                        .bytes()
                        .any(|b| !(b >= 32 && b != 127 || b == b'\t')),
                    "error reported but no forbidden byte found in: {:?}",
                    value_str
                );
            }
        }
    }

    // Fuzz HeaderBlock operations
    let count = (block_byte as usize) % 16;
    let mut block = HeaderBlock::new();
    for i in 0..count {
        let name_str = format!("x-{}-{}", block_byte, i);
        let value_str = format!("v-{}-{}", block_byte, i);
        if let (Ok(name), Ok(value)) = (HeaderName::new(&name_str), HeaderValue::new(&value_str)) {
            block.push(name, value);
        }
    }

    let lookup_name = format!("x-{}-0", lookup_byte);
    let _ = block.get_first(&lookup_name);
    let _ = block.get_all(&lookup_name);
    let _ = block.get_unique(&lookup_name);
    let _ = block.contains(&lookup_name);

    let mut prev_index = None;
    for (idx, field) in block.iter().enumerate() {
        if let Some(prev) = prev_index {
            assert!(idx > prev);
        }
        prev_index = Some(idx);
        assert!(!field.name.as_str().is_empty());
    }
});
