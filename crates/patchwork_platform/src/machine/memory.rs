//! Keeping what Patchwork holds in memory out of core dumps and out of the swap file.

// Rust guideline compliant 2026-10-09

/// Keeps this process's memory out of core dumps.
///
/// Sets `RLIMIT_CORE` to zero instead of clearing `PR_SET_DUMPABLE`: on Linux, clearing
/// `PR_SET_DUMPABLE` hides `/proc/<pid>/root` from `xdg-desktop-portal`, which rejects file
/// dialogs and other portal requests with `AccessDenied`. On Unix only; elsewhere this does
/// nothing.
///
/// # Errors
///
/// Returns the system's error when the core-dump limit cannot be read or lowered.
pub fn deny_dumps() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `limit` is a valid, aligned `rlimit` struct written by `getrlimit`.
        if unsafe { libc::getrlimit(libc::RLIMIT_CORE, &raw mut limit) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        limit.rlim_cur = 0;
        // SAFETY: `limit` is a valid `rlimit` struct read by `setrlimit`.
        if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &raw const limit) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Keeps this process on small memory pages, so the memory it reports is the memory it uses.
///
/// Where the system hands out huge pages unasked, as many Linux systems do, the allocator's
/// mostly empty blocks are each backed by a whole two-megabyte page, about a quarter of the app's
/// memory. The programs Patchwork starts keep the setting. On Linux only; elsewhere this does
/// nothing.
///
/// # Errors
///
/// Returns the system's error when the setting cannot be changed.
pub fn small_pages() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `PR_SET_THP_DISABLE` takes numbers, the unused ones zero, and reads no memory
        // of ours.
        let set = unsafe {
            libc::prctl(
                libc::PR_SET_THP_DISABLE,
                libc::c_ulong::from(1_u8),
                libc::c_ulong::from(0_u8),
                libc::c_ulong::from(0_u8),
                libc::c_ulong::from(0_u8),
            )
        };
        if set != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Keeps the memory `value` is in from being written to the swap file.
///
/// The pages stay in memory for as long as Patchwork runs, so `value` should be something that
/// does not move, such as the inside of a `Box`. On Unix only; elsewhere this does nothing.
///
/// # Errors
///
/// Returns the system's error when the pages cannot be kept, as when the account's limit on
/// such memory is reached.
pub fn pin<T>(value: &T) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let start = std::ptr::from_ref(value).cast::<libc::c_void>();
        // SAFETY: the range is `value`, which is borrowed and so mapped, and `mlock` neither
        // reads nor writes it.
        if unsafe { libc::mlock(start, size_of::<T>()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    #[cfg(not(unix))]
    let _ = value;
    Ok(())
}
