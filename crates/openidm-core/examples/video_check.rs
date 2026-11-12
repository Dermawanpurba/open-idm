//! Manual live check; all output goes to the supplied test folder, never the user's queue.
use std::collections::BTreeMap;
use std::path::PathBuf;
use openidm_core::engine::{Job, Outcome, Progress};
use openidm_core::video::Tools;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 { return Err("usage: video_check <helper-root> <output-directory> <URL> [quality]".into()); }
    let tools = Tools::at(&PathBuf::from(&args[1]));
    let info = tools.info(&args[3]).await?;
    println!("INFO: {} — {} choices", info.title, info.formats.len());
    assert!(info.formats.len() > 1, "quality choices must be real");
    let output = PathBuf::from(&args[2]);
    tokio::fs::create_dir_all(&output).await?;
    let job = Job { id: "live-video-check".into(), url: args[3].parse()?, headers: BTreeMap::new(),
        final_path: output.join("verified-video.mp4"), part_path: output.join("verified-video.mp4.part"), expected_size: 0 };
    let progress = Progress::default();
    let outcome = tools.download(&job, args.get(4).map(String::as_str).unwrap_or("video:144"), 8, &progress, CancellationToken::new(), None).await?;
    match outcome {
        Outcome::Completed { path, size } => {
            assert!(size > 1000);
            let bytes = tokio::fs::read(&path).await?;
            assert_eq!(&bytes[4..8], b"ftyp", "must be MP4, not downloaded HTML");
            println!("PASS: real video + audio saved: {} ({} bytes)", path.display(), size);
        }
        Outcome::Paused => return Err("unexpected pause".into()),
    }
    Ok(())
}
