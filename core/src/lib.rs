pub mod diff;
pub mod engine;
pub mod insights;
pub mod sentence;
pub mod settings;
pub mod spans;
pub mod text;

#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("ugrammar-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
