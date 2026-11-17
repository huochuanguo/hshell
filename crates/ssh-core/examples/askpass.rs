fn main() {
    std::process::exit(ssh_core::run_askpass_helper().unwrap_or(1));
}
