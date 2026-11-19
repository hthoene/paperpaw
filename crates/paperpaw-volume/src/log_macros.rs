//! Logging is compiled out: the arguments are type-checked, never evaluated.

macro_rules! log_nothing {
    ($($arg:tt)+) => {
        if false {
            let _ = ::core::format_args!($($arg)+);
        }
    };
}
macro_rules! error { ($($arg:tt)+) => { log_nothing!($($arg)+) }; }
macro_rules! warn { ($($arg:tt)+) => { log_nothing!($($arg)+) }; }
macro_rules! info { ($($arg:tt)+) => { log_nothing!($($arg)+) }; }
macro_rules! debug { ($($arg:tt)+) => { log_nothing!($($arg)+) }; }
macro_rules! trace { ($($arg:tt)+) => { log_nothing!($($arg)+) }; }
