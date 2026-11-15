#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod host;
mod ui;
mod workers;
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("WifiMap: {error}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mut initial = None;
    let mut diagnostics = None;
    let mut update_failed = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--apply-update")=>{
                let path=PathBuf::from(args.next().ok_or("Missing update job path")?);
                let result=wifi_map::updates::apply_install(&path);
                if let Err(error)=&result {
                    let _=wifi_map::persistence::atomic_write(&path.with_extension("error.txt"),error.to_string().as_bytes());
                }
                result?;
                return Ok(());
            }
            Some("--peer-server")=>{
                let address=args.next().ok_or("Missing peer IP:port")?;
                let token=args.next().ok_or("Missing 32-character shared token")?;
                wifi_map::peer::serve_once(wifi_map::peer::local_address(&address.to_string_lossy())?,wifi_map::peer::parse_token(&token.to_string_lossy())?)?;
                return Ok(());
            }
            Some("--open")=>initial=Some(PathBuf::from(args.next().ok_or("Missing project path")?)),
            Some("--diagnostics")=>diagnostics=Some(PathBuf::from(args.next().ok_or("Missing diagnostics output path")?)),
            Some("--update-failed")=>update_failed=Some(PathBuf::from(args.next().ok_or("Missing update result path")?)),
            _=>return Err("Unknown argument. Use --open PROJECT, --diagnostics PATH or --peer-server IP:PORT TOKEN.".into()),
        }
    }
    host::run(initial, diagnostics, update_failed)
}
