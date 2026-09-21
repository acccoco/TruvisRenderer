use truvis_app::desktop::TruvisDesktop;

fn main() {
    if let Err(error) = TruvisDesktop::run() {
        eprintln!("failed to run Truvis Tauri desktop: {error}");
        std::process::exit(1);
    }
}
