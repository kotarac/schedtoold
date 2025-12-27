use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read};
use std::process;
use std::thread;
use std::time;

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

fn get_config(path: &str) -> Result<Config> {
    let content =
        fs::read_to_string(path).with_context(|| format!("couldn't read config file: {path}"))?;
    let config: Config =
        ron::de::from_str(&content).with_context(|| format!("couldn't parse config: {path}"))?;

    if config.version != CONFIG_VERSION {
        bail!(
            "config version mismatch: current is {}, config file is {}",
            CONFIG_VERSION,
            config.version
        );
    }

    Ok(config)
}

fn is_match(exe: &str, cmdline: &str, name: &str) -> bool {
    exe.ends_with(name) || cmdline.ends_with(name)
}

fn get_cmdline_by_pid(pid: u32, path_buf: &mut String, content_buf: &mut String) {
    path_buf.clear();
    let _ = write!(path_buf, "/proc/{pid}/cmdline");
    content_buf.clear();
    if let Ok(mut file) = File::open(&path_buf) {
        let _ = file.read_to_string(content_buf);
        if let Some(idx) = content_buf.find('\0') {
            content_buf.truncate(idx);
        }
    }
}

fn get_exe_by_pid(pid: u32, path_buf: &mut String, exe_buf: &mut String) {
    path_buf.clear();
    let _ = write!(path_buf, "/proc/{pid}/exe");
    exe_buf.clear();
    if let Ok(path) = fs::read_link(&path_buf) {
        if let Some(s) = path.to_str() {
            exe_buf.push_str(s);
        }
    }
}

fn update_pids_current(pids: &mut HashSet<u32>) -> io::Result<()> {
    pids.clear();
    for entry in fs::read_dir("/proc")?.flatten() {
        if let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        {
            if pid != 1 && pid != process::id() {
                pids.insert(pid);
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    let config = get_config(&args.config)?;
    let interval = time::Duration::from_millis(args.interval);

    let mut pids_processed: HashSet<u32> = HashSet::new();
    let mut pids_current: HashSet<u32> = HashSet::new();
    let mut exe_buf = String::with_capacity(256);
    let mut cmdline_buf = String::with_capacity(256);
    let mut path_buf = String::with_capacity(64);

    loop {
        update_pids_current(&mut pids_current).context("couldn't get pids")?;

        for pid in pids_current.difference(&pids_processed) {
            get_exe_by_pid(*pid, &mut path_buf, &mut exe_buf);
            get_cmdline_by_pid(*pid, &mut path_buf, &mut cmdline_buf);

            for (name, flags) in &config.items {
                if !is_match(&exe_buf, &cmdline_buf, name) {
                    continue;
                }

                if args.verbose {
                    println!(
                        "matched pid = {pid:?}, exe = {exe_buf:?}, cmdline = {cmdline_buf:?}, name = {name:?}, flags = {flags:?}"
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
                            eprintln!("pid {pid} schedtool failed with flags {flags}: {status:?}");
                        }
                    }
                    Err(e) => {
                        eprintln!("pid {pid} failed to execute schedtool with flags {flags}: {e}");
                    }
                }
            }
        }

        std::mem::swap(&mut pids_processed, &mut pids_current);

        thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_get_config_valid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"Config(version: 1, items: [("pipewire", "-F -p 4")])"#
        )
        .unwrap();
        let path = file.path().to_str().unwrap();
        let config = get_config(path).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.items.len(), 1);
        assert_eq!(config.items[0].0, "pipewire");
        assert_eq!(config.items[0].1, "-F -p 4");
    }

    #[test]
    fn test_get_config_version_mismatch() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"Config(version: 2, items: [])"#).unwrap();
        let path = file.path().to_str().unwrap();
        assert!(get_config(path).is_err());
    }

    #[test]
    fn test_get_config_invalid_ron() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "invalid ron").unwrap();
        let path = file.path().to_str().unwrap();
        assert!(get_config(path).is_err());
    }

    #[test]
    fn test_get_config_empty_items() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"Config(version: 1, items: [])"#).unwrap();
        let path = file.path().to_str().unwrap();
        let config = get_config(path).unwrap();
        assert_eq!(config.version, 1);
        assert!(config.items.is_empty());
    }

    #[test]
    fn test_get_config_with_comments() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"Config(
                // a comment
                version: 1,
                /* another comment */
                items: [
                    ("app", "-p 1")
                ]
            )"#
        )
        .unwrap();
        let path = file.path().to_str().unwrap();
        let config = get_config(path).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.items.len(), 1);
    }

    #[test]
    fn test_is_match() {
        assert!(is_match("/usr/bin/pipewire", "", "pipewire"));
        assert!(is_match("", "sway", "sway"));
        assert!(is_match("/usr/bin/Xorg", "Xorg", "Xorg"));
        assert!(!is_match("/usr/bin/chrome", "chrome", "pipewire"));
        assert!(is_match("some/path/wireplumber", "", "wireplumber"));
        assert!(is_match("pipewire", "", "pipewire"));
        assert!(is_match("", "pipewire", "pipewire"));
        assert!(!is_match("pipewire-plugin", "", "pipewire"));
        assert!(!is_match("", "pipewire-plugin", "pipewire"));
        assert!(!is_match("/usr/bin/PIPEWIRE", "", "pipewire"));
        assert!(!is_match("", "", "anything"));
        assert!(is_match("", "", ""));
        assert!(is_match("path/to/my-app", "", "app"));
    }
}
