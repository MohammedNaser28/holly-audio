//! holly — CLI front end for the Hollyland 6108 dongle (3547:0407).
//!
//! Commands: exactly `status`, `nc`, `watch`. Nothing else by rule
//! (no OTA/reboot/pairing/reset subcommands may ever be added here).
//! Exit codes: 0 ok · 1 other · 2 device not found · 3 permission denied ·
//! 4 protocol/ack error.
//!
//! The clap schema lives in the lib half (holly_cli::{Cli, Command}) for
//! testability; this binary only parses, wires tracing, and dispatches.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::Parser;
use tracing::debug;
use tracing_subscriber::EnvFilter;

use holly_cli::{Cli, Command, EXIT_PERMISSION, Failure, NcState, Watcher, status_line};
use holly_usb::{HollyControl, NusbTransport};

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    debug!(?cli.command, verbose = cli.verbose, "parsed arguments");
    std::process::ExitCode::from(run(cli) as u8)
}

fn init_tracing(verbose: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(if verbose { "debug" } else { "warn" }));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn run(cli: Cli) -> i32 {
    match cli.command {
        Command::Status { json } => {
            let mut ctl = open_or_exit(cli.verbose);
            match holly_cli::run_status(&mut ctl, json) {
                Ok(text) => {
                    println!("{text}");
                    0
                }
                Err(e) => fail(&e, cli.verbose),
            }
        }
        Command::Nc { state } => {
            let on = state == NcState::On;
            let mut ctl = open_or_exit(cli.verbose);
            match holly_cli::run_nc(&mut ctl, on) {
                Ok(info) => {
                    println!(
                        "noise-cancel is now {} (level {})",
                        holly_cli::onoff(info.noise_switch),
                        info.noise_level
                    );
                    0
                }
                Err(holly_cli::NcError::Usb(e)) => fail(&e, cli.verbose),
                Err(holly_cli::NcError::Mismatch(info)) => {
                    eprintln!(
                        "protocol error: device reported NC = {} after an acknowledged SET to {}",
                        holly_cli::onoff(info.noise_switch),
                        holly_cli::onoff(on)
                    );
                    if cli.verbose {
                        eprintln!("device state: {}", status_line(&info));
                    }
                    holly_cli::EXIT_PROTOCOL
                }
            }
        }
        Command::Watch { interval_ms } => {
            let ctl = open_or_exit(cli.verbose);
            run_watch(ctl, interval_ms, cli.verbose)
        }
    }
}

/// Open + claim IF0, or print the mapped failure and exit non-zero.
fn open_or_exit(verbose: bool) -> HollyControl<NusbTransport> {
    match HollyControl::open() {
        Ok(ctl) => ctl,
        Err(e) => {
            let code = fail(&e, verbose);
            std::process::exit(code);
        }
    }
}

fn run_watch(ctl: HollyControl<NusbTransport>, interval_ms: u64, _verbose: bool) -> i32 {
    // verbose currently changes nothing inside the watch loop: the poll line
    // is already the detail. (Kept in the signature to match the other
    // commands' shapes; `_verbose` silences clippy until tracing hooks land.)
    let stopped = Arc::new(AtomicBool::new(false));
    if let Err(e) = ctrlc::set_handler({
        let stopped = stopped.clone();
        move || stopped.store(true, Ordering::Relaxed)
    }) {
        eprintln!("could not install Ctrl-C handler: {e:?}");
        return holly_cli::EXIT_OTHER;
    }
    let mut w = Watcher::new(ctl);
    let interval = Duration::from_millis(interval_ms);
    debug!("watching with interval {interval_ms} ms");
    loop {
        if stopped.load(Ordering::Relaxed) {
            debug!("Ctrl-C received; clean exit");
            return 0;
        }
        match w.poll() {
            Ok(Some(line)) => println!("{line}"),
            Ok(None) => {}
            Err(e) => {
                let f = Failure::from(&e);
                eprintln!("error: {}", f.line);
                eprintln!("watch stopped (poll failed); rerun to resume");
                return f.code;
            }
        }
        std::thread::sleep(interval);
    }
}

fn fail(e: &holly_usb::Error, verbose: bool) -> i32 {
    let f = Failure::from(e);
    eprintln!("error: {}", f.line);
    if f.code == EXIT_PERMISSION {
        // the udev-rule hint is inside the message; remind about the replug
        // so the logind ACL actually applies
        eprintln!("(then unplug and replug the dongle so the ACL applies)");
    }
    if verbose && let Some(detail) = &f.detail {
        eprintln!("raw frame: {detail}");
    }
    f.code
}
