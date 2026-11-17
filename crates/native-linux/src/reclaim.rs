//! Return free allocator pages after terminals have been closed.
//! This is delayed and off the GTK thread, never part of the input/output path.
#[cfg(feature = "native-probe")]
static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "native-probe")]
pub fn calls() -> usize {
    CALLS.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(target_env = "gnu")]
pub fn idle_pages() {
    #[cfg(feature = "native-probe")]
    if std::env::var_os("HSHELL_NATIVE_NO_RECLAIM").is_some() {
        return;
    }
    std::thread::spawn(|| {
        extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // glibc's thread-safe trim returns only pages belonging to free chunks.
        unsafe {
            malloc_trim(0);
        }
        #[cfg(feature = "native-probe")]
        CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
}

#[cfg(not(target_env = "gnu"))]
pub fn idle_pages() {}
