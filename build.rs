fn main() {
    // The Slint UI is only compiled when the `gui` feature is enabled, so a
    // library-only build (default-features = false) needs neither slint-build
    // nor a display toolkit.
    #[cfg(feature = "gui")]
    slint_build::compile("ui/app.slint").expect("Slint compilation failed");
}
