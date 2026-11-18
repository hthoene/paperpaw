//! The kernel's own setting groups are kept in the program, not copied onto
//! the heap (#248, B): each is a `static` the kernel registers by reference.

extern crate std;

use super::*;

/// The address ranges of this test program's own mappings: what lies outside
/// them is on the heap.
#[cfg(target_os = "linux")]
fn program() -> Vec<(usize, usize)> {
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let exe = exe.to_string_lossy().into_owned();
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    maps.lines()
        .filter(|line| line.ends_with(exe.as_str()))
        .filter_map(|line| {
            let (start, end) = line.split_whitespace().next()?.split_once('-')?;
            Some((
                usize::from_str_radix(start, 16).ok()?,
                usize::from_str_radix(end, 16).ok()?,
            ))
        })
        .collect()
}

#[cfg(target_os = "linux")]
#[test]
fn the_kernels_setting_groups_are_kept_in_the_program() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    let program = program();
    assert!(!program.is_empty());
    let mut copied = Vec::new();
    let mut kernels = 0;
    for group in kernel.settings().groups() {
        if group.id.namespace() != "core" {
            continue;
        }
        kernels += 1;
        let at = core::ptr::from_ref(group) as usize;
        if !program
            .iter()
            .any(|(start, end)| (*start..*end).contains(&at))
        {
            copied.push(group.id.as_str());
        }
    }
    assert!(kernels > 10, "{kernels} groups");
    assert!(copied.is_empty(), "copied onto the heap: {copied:?}");
}
