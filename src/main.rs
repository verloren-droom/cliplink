fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt()
            .with_target(false)
            .compact()
            .init();
    }

    cliplink::run_native_ui_app()
}
