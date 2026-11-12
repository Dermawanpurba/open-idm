// stdout carries Native Messaging frames only: never print to it.
#![windows_subsystem = "console"]

use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::Duration;

use openidm_core::protocol::write_frame;
use openidm_core::winutil::pipe_name;
use openidm_host::relay::{pump_stdin, relay, Connector};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::sync::mpsc;

const APP_WAIT: Duration = Duration::from_secs(5);
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

struct PipeConnector;

impl Connector for PipeConnector {
    type Conn = NamedPipeClient;

    async fn connect(&mut self) -> io::Result<NamedPipeClient> {
        ClientOptions::new().open(pipe_name()?)
    }

    fn launch_app(&mut self) -> io::Result<()> {
        let exe = std::env::current_exe()?.with_file_name("OpenIDM.exe");
        let spawn = |flags: u32| {
            Command::new(&exe)
                .arg("--background")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(flags)
                .spawn()
        };
        // Prefer leaving the browser's job object so the app outlives this host; fall back if not allowed.
        spawn(DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
            .or_else(|_| spawn(DETACHED_PROCESS | CREATE_NO_WINDOW))
            .map(|_| ())
    }
}

fn main() {
    let (tx, rx) = mpsc::channel(64);
    std::thread::spawn(move || pump_stdin(io::stdin().lock(), tx));
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let stdout = io::stdout();
    let _ = runtime.block_on(relay(
        rx,
        |frame: &[u8]| {
            let mut out = stdout.lock();
            write_frame(&mut out, frame)?;
            out.flush()
        },
        &mut PipeConnector,
        APP_WAIT,
    ));
}
