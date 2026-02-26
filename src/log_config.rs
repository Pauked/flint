use std::io;

use log::LevelFilter;
use log4rs::{
    append::{
        console::{ConsoleAppender, Target},
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

pub fn init(verbose: u8) {
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

    // Suppress noisy third-party crate logs (CSS parsing, Chrome DevTools, cookies, etc.)
    let noisy_crates = [
        "selectors",
        "html5ever",
        "cssparser",
        "reqwest",
        "hyper",
        "headless_chrome",
        "tungstenite",
        "cookie_store",
    ];
    for name in noisy_crates {
        config_builder = config_builder.logger(Logger::builder().build(name, LevelFilter::Warn));
    }

    let config = config_builder
        .build(root_builder.build(LevelFilter::Trace))
        .unwrap();

    let _ = log4rs::init_config(config);
}
