use std::path::Path;

pub fn replay_from(path: &str) -> anyhow::Result<()> {
    let snapshot_path = Path::new(path);
    if snapshot_path.exists() {
        println!("replaying snapshots from {:?}", snapshot_path);
    } else {
        println!("no snapshots found at {:?}, continuing", snapshot_path);
    }
    Ok(())
}
