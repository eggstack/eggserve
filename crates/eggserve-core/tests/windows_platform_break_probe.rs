#![cfg(windows)]

// TEMPORARY Plan 312 gate demonstration. This file must never be committed
// to main: it exists only to prove that the `platform-compile` CI job
// catches a Windows-only compile break.

#[test]
fn deliberate_platform_break_probe() {
    let _: u32 = "this is deliberately not a u32";
}
