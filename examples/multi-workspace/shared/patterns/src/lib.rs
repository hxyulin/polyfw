#![no_std]

use timing::Millis;

/// Reusable on/off durations for a buzzer or LED.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PulsePattern {
    pub on: Millis,
    pub off: Millis,
}

pub const FAST: PulsePattern = PulsePattern {
    on: Millis(100),
    off: Millis(100),
};

pub const SLOW: PulsePattern = PulsePattern {
    on: Millis(500),
    off: Millis(500),
};
