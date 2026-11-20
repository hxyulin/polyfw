#![no_std]

pub fn message() -> (&'static str, &'static str) {
    (common::message(), second_only::message())
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
