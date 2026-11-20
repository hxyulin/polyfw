#![no_std]

pub enum BlinkRate {
    Slow,
    Fast,
}

pub fn blink_pattern(rate: BlinkRate) -> patterns::PulsePattern {
    match rate {
        BlinkRate::Slow => patterns::SLOW,
        BlinkRate::Fast => patterns::FAST,
    }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
