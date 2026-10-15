#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    arkline_client::run_native()
}
#[cfg(target_arch = "wasm32")]
fn main() {
    arkline_client::run_browser();
}
