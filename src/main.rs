use clap::Parser;
use serde::Deserialize;
use std::fs;
use std::io;
use std::process;
use std::thread;
use std::time;
use std::collections::HashSet;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// process checking interval in milliseconds
    #[arg(short, long, default_value_t = 2000)]
    interval: u64,

    /// config file path
    #[arg(short, long, default_value_t = String::from("/etc/schedtoold.ron"), value_hint = clap::ValueHint::FilePath)]
    config: String,

    /// verbose output (prints matches)
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
pub struct Config {
    version: u32,
    items: Vec<(String, String)>,
}

fn get_config(path: &str) -> Config {
    let config: Config =
        ron::de::from_str(&fs::read_to_string(path).expect("couldn't read config file"))
            .expect("couldn't parse config, check syntax");

    if config.version != CONFIG_VERSION {
        panic!(
            "config version mismatch: current is {}, config file is {}",
            CONFIG_VERSION, config.version
        );
    }

    config
}

fn get_cmdline_by_pid(pid: &u32) -> String {
    match fs::read_to_string(format!("/proc/{}/cmdline", pid))
        .unwrap_or(String::new())
        .split_once('\0')
    {
        Some((basename, _rest)) => basename.to_string(),
        _ => String::new(),
    }
}

fn get_exe_by_pid(pid: &u32) -> String {
    match fs::read_link(format!("/proc/{}/exe", pid)) {
        Ok(path) => path.into_os_string().into_string().unwrap_or(String::new()),
        _ => String::new(),
    }
}

fn get_pids_current() -> io::Result<HashSet<u32>> {
    Ok(fs::read_dir("/proc")?
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
        })
        .filter(|pid| pid != &1 && pid != &process::id())
        .collect::<HashSet<u32>>())
}

fn main() {
    let args = Args::parse();

    let config = get_config(&args.config);
    let interval = time::Duration::from_millis(args.interval);

    let mut pids_processed: HashSet<u32> = HashSet::new();

    loop {
        let pids_current = get_pids_current().expect("couldn't get pids");

        for pid in pids_current.difference(&pids_processed) {
            let exe = get_exe_by_pid(pid);
            let cmdline = get_cmdline_by_pid(pid);

            for (name, flags) in &config.items {
                if !exe.ends_with(name) && !cmdline.ends_with(name) {
                    continue;
                }

                if args.verbose {
                    println!(
                        "matched pid = {:?}, exe = {:?}, cmdline = {:?}, name = {:?}, flags = {:?}",
                        pid, exe, cmdline, name, flags
                    );
                }

                let status = process::Command::new("schedtool")
                    .args(flags.split_whitespace())
                    .arg(format!("{}", *pid))
                    .stdout(process::Stdio::null())
                    .stderr(process::Stdio::null())
                    .status();

                match status {
                    Ok(status) => {
                        if !status.success() {
                            eprintln!("pid {} schedtool failed with flags {}: {:?}", pid, flags, status);
                        }
                    }
                    Err(e) => {
                        eprintln!("pid {} failed to execute schedtool with flags {}: {}", pid, flags, e);
                    }
                }
            }
        }

        pids_processed = pids_current;

        thread::sleep(interval);
    }
}
