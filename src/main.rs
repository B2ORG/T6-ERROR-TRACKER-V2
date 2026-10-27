mod addresses;
mod display;
mod dump;
mod memory;
mod options;
mod tracker;

use display::Dashboard;
use memory::Process;
use options::Options;
use std::{process::ExitCode, thread, time::Duration};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[{}] ERROR {e}", display::timestamp());
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let Some(options) = Options::parse(std::env::args().skip(1))? else {
        print!("{}", options::HELP);
        return Ok(ExitCode::SUCCESS);
    };
    let mut dashboard = Dashboard::new(&options);
    dashboard.message(
        "INFO",
        &format!("T6 Error Tracker v{} started", env!("CARGO_PKG_VERSION")),
    )?;
    match track(&options, &mut dashboard) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            dashboard.status("Stopped; last readings retained");
            dashboard.message("ERROR", &error)?;
            Ok(ExitCode::FAILURE)
        }
    }
}

fn track(o: &Options, dashboard: &mut Dashboard) -> Result<(), String> {
    let mut samples = 0u64;
    let mut previous_status = String::new();
    loop {
        let candidates = if let Some(pid) = o.pid {
            vec![(pid, "explicit PID".to_owned())]
        } else {
            Process::discover()?
        };
        let mut attached = None;
        let mut status = String::from("Waiting for T6 Zombies process");
        for (pid, name) in candidates {
            match Process::open(pid, name).and_then(|p| {
                p.validate()?;
                Ok(p)
            }) {
                Ok(p) => {
                    attached = Some(p);
                    break;
                }
                Err(e) => status = format!("Cannot attach to PID {pid}: {e}"),
            }
        }
        let Some(process) = attached else {
            dashboard.status("Waiting for T6 process; no live sample");
            if status != previous_status {
                dashboard.message(
                    if status.starts_with("Cannot") {
                        "WARN"
                    } else {
                        "INFO"
                    },
                    &status,
                )?;
                previous_status = status;
            }
            if o.pid.is_some() {
                return Err(previous_status);
            }
            thread::sleep(Duration::from_secs(1));
            continue;
        };
        previous_status.clear();
        dashboard.status("Attached; awaiting first sample");
        dashboard.message(
            "INFO",
            &format!(
                "Attached to {} (PID {}); verified T6 Zombies server address profile",
                process.name, process.pid
            ),
        )?;
        let mut peaks = tracker::Peaks::default();
        let mut bad_samples = 0;
        let mut previous_state = None;
        let mut reset_warning = false;
        loop {
            match tracker::sample(&process, o, &mut peaks) {
                Ok(s) => {
                    bad_samples = 0;
                    if s.active || previous_state != Some(false) || dashboard.interactive() {
                        dashboard.sample(&s, o)?;
                    }
                    if previous_state != Some(s.active) {
                        dashboard.message("INFO", if s.active { "Local server/map active" } else { "Waiting for a local server/map; server values are unavailable on a remote client" })?;
                    }
                    previous_state = Some(s.active);
                    let at_limit = s.reset.as_ref().is_some_and(|r| r.remaining == 0);
                    if at_limit && !reset_warning {
                        dashboard.message("WARN", "Server snapshot reset threshold reached")?;
                    }
                    reset_warning = at_limit;
                    if let Some(kind) = o.dump
                        && s.active
                    {
                        let path = dump::dump(&process, kind)?;
                        dashboard.status("Completed");
                        dashboard.message("INFO", &format!("Wrote {}", path.display()))?;
                        return Ok(());
                    }
                    if s.active {
                        samples += 1;
                        if o.samples.is_some_and(|limit| samples >= limit) {
                            dashboard.status("Completed");
                            dashboard
                                .message("INFO", &format!("Captured {samples} live samples"))?;
                            return Ok(());
                        }
                    }
                }
                Err(error) => {
                    dashboard.status("Sample unavailable; last readings retained");
                    dashboard.message("WARN", &format!("Sample unavailable: {error}"))?;
                    previous_state = None;
                    bad_samples += 1;
                    if bad_samples >= 3 {
                        if o.pid.is_some() {
                            return Err(error);
                        }
                        dashboard.status("Reacquiring T6 process; last readings retained");
                        dashboard.message("INFO", "Reacquiring T6 process")?;
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(o.interval));
        }
        thread::sleep(Duration::from_secs(1));
    }
}
