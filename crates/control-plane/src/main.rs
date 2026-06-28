use olympus_control_plane as _;

fn main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();
    tracing::info!("olympus control plane starting");
    println!("olympus control plane — placeholder");
}
