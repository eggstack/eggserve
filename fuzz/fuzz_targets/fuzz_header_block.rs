#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let count = (data[0] as usize) % 16;
    let key_byte = data[1];
    let val_byte = data[2];
    let lookup_byte = data[3];

    let mut block = HeaderBlock::new();
    for i in 0..count {
        let name_str = format!("x-{}-{}", key_byte, i);
        let value_str = format!("v-{}-{}", val_byte, i);
        if let (Ok(name), Ok(value)) = (HeaderName::new(&name_str), HeaderValue::new(&value_str))
        {
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
