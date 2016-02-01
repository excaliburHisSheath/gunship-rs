#![feature(op_assign_traits)]
#![cfg_attr(windows, feature(str_utf16))]

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::init::init;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::init::init;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::init::init;

pub mod window;
pub mod input;
pub mod time;
