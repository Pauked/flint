use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;
use log::LevelFilter;
use log4rs::{
    Handle,
    append::{
        console::{ConsoleAppender, Target},
        file::FileAppender,
        rolling_file::{
            RollingFileAppender,
            policy::compound::{
                CompoundPolicy, roll::fixed_window::FixedWindowRoller, trigger::size::SizeTrigger,
            },
        },
    },
    config::{Appender, Logger, Root},
    encode::{self, Encode, pattern::PatternEncoder},
    filter::threshold::ThresholdFilter,
};

/// Custom encoder that applies ANSI colors based on log level.
/// Error → red, Warn → yellow, Debug/Trace → dim grey, Info → no color.
#[derive(Debug)]
struct ColorEncoder;

impl Encode for ColorEncoder {
    fn encode(&self, w: &mut dyn encode::Write, record: &log::Record) -> anyhow::Result<()> {
        match record.level() {
            log::Level::Error => {
                write!(w, "\x1b[31m{}\x1b[0m", record.args())?;
            }
            log::Level::Warn => {
                write!(w, "\x1b[33m{}\x1b[0m", record.args())?;
            }
            log::Level::Debug | log::Level::Trace => {
                write!(w, "\x1b[90m{}\x1b[0m", record.args())?;
            }
            log::Level::Info => {
                write!(w, "{}", record.args())?;
            }
        }
        writeln!(w)?;
        Ok(())
    }
}

// Blanket impl required by log4rs — our encoder doesn't use io::Write directly,
// but ConsoleAppender wraps its output in a type that implements encode::Write.
impl io::Write for ColorEncoder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Noisy third-party crates to suppress
const NOISY_CRATES: &[&str] = &[
    "selectors",
    "html5ever",
    "cssparser",
    "reqwest",
    "hyper",
    "headless_chrome",
    "tungstenite",
    "cookie_store",
];

pub fn init(verbose: u8) -> Handle {
    let console_level = match verbose {
        0 => LevelFilter::Info,
        1 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    let stderr = ConsoleAppender::builder()
        .encoder(Box::new(ColorEncoder))
        .target(Target::Stderr)
        .build();

    let mut config_builder = log4rs::config::Config::builder().appender(
        Appender::builder()
            .filter(Box::new(ThresholdFilter::new(console_level)))
            .build("stderr", Box::new(stderr)),
    );

    let mut root_builder = Root::builder().appender("stderr");

    // Add rolling file appender when verbose mode is enabled
    if verbose > 0 {
        let temp_dir = std::env::temp_dir();
        let log_path = temp_dir.join("flint.log");
        let roller_pattern = temp_dir.join("flint_history{}.log");

        let trigger = Box::new(SizeTrigger::new(1024 * 1024 * 3)); // 3MB
        let roller = Box::new(
            FixedWindowRoller::builder()
                .base(1)
                .build(roller_pattern.to_str().unwrap(), 3)
                .unwrap(),
        );
        let policy = Box::new(CompoundPolicy::new(trigger, roller));
        let logfile = RollingFileAppender::builder()
            .encoder(Box::new(PatternEncoder::new(
                "[ {d(%Y-%m-%d %H:%M:%S)(utc)} | {l:5.5} ] {m}{n}",
            )))
            .build(log_path, policy)
            .unwrap();

        config_builder =
            config_builder.appender(Appender::builder().build("logfile", Box::new(logfile)));
        root_builder = root_builder.appender("logfile");
    }

    for name in NOISY_CRATES {
        config_builder = config_builder.logger(Logger::builder().build(*name, LevelFilter::Warn));
    }

    let config = config_builder
        .build(root_builder.build(LevelFilter::Trace))
        .unwrap();

    log4rs::init_config(config).expect("Failed to initialize logging")
}

/// Add a per-run log file appender. Returns the log file path.
pub fn init_run_log(handle: &Handle, verbose: u8, log_dir: &Path) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(log_dir)?;

    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
    let log_path = log_dir.join(format!("flint_{timestamp}.log"));

    let console_level = match verbose {
        0 => LevelFilter::Info,
        1 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    // Rebuild config with console + run log (and verbose rolling file if enabled)
    let stderr = ConsoleAppender::builder()
        .encoder(Box::new(ColorEncoder))
        .target(Target::Stderr)
        .build();

    let run_log = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "[ {d(%Y-%m-%d %H:%M:%S)(utc)} | {l:5.5} ] {m}{n}",
        )))
        .build(&log_path)?;

    let mut config_builder = log4rs::config::Config::builder()
        .appender(
            Appender::builder()
                .filter(Box::new(ThresholdFilter::new(console_level)))
                .build("stderr", Box::new(stderr)),
        )
        .appender(Appender::builder().build("runlog", Box::new(run_log)));

    let mut root_builder = Root::builder().appender("stderr").appender("runlog");

    if verbose > 0 {
        let temp_dir = std::env::temp_dir();
        let rolling_path = temp_dir.join("flint.log");
        let roller_pattern = temp_dir.join("flint_history{}.log");

        let trigger = Box::new(SizeTrigger::new(1024 * 1024 * 3));
        let roller = Box::new(
            FixedWindowRoller::builder()
                .base(1)
                .build(roller_pattern.to_str().unwrap(), 3)
                .unwrap(),
        );
        let policy = Box::new(CompoundPolicy::new(trigger, roller));
        let logfile = RollingFileAppender::builder()
            .encoder(Box::new(PatternEncoder::new(
                "[ {d(%Y-%m-%d %H:%M:%S)(utc)} | {l:5.5} ] {m}{n}",
            )))
            .build(rolling_path, policy)?;

        config_builder =
            config_builder.appender(Appender::builder().build("logfile", Box::new(logfile)));
        root_builder = root_builder.appender("logfile");
    }

    for name in NOISY_CRATES {
        config_builder = config_builder.logger(Logger::builder().build(*name, LevelFilter::Warn));
    }

    let config = config_builder
        .build(root_builder.build(LevelFilter::Trace))
        .unwrap();

    handle.set_config(config);

    Ok(log_path)
}

/// Delete `.log` files in `log_dir` older than `keep_days`.
pub fn cleanup_old_logs(log_dir: &Path, keep_days: u32) {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(keep_days));

    let entries = match fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let modified = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let modified_dt: chrono::DateTime<chrono::Utc> = modified.into();
        if modified_dt < cutoff {
            if let Err(e) = fs::remove_file(&path) {
                log::warn!("Failed to remove old log file {}: {e}", path.display());
            } else {
                log::debug!("Removed old log file: {}", path.display());
            }
        }
    }
}
