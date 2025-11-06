use std::{io::Write, sync::Mutex};

struct Logger<Writer> {
    max_level: log::LevelFilter,
    output: Writer,
    lock: Mutex<()>,
}

impl Logger<()> {
    const fn new() -> Self {
        Self {
            max_level: log::LevelFilter::Off,
            output: (),
            lock: Mutex::new(()),
        }
    }
}

impl<T> Logger<T> {
    fn level(mut self, max_level: log::LevelFilter) -> Self {
        log::set_max_level(max_level);
        self.max_level = max_level;
        self
    }

    fn increase_level(mut self) -> Self {
        let new_level = match self.max_level {
            log::LevelFilter::Off => log::LevelFilter::Error,
            log::LevelFilter::Error => log::LevelFilter::Warn,
            log::LevelFilter::Warn => log::LevelFilter::Info,
            log::LevelFilter::Info => log::LevelFilter::Debug,
            log::LevelFilter::Debug => log::LevelFilter::Trace,
            _ => log::LevelFilter::Trace,
        };
        self.max_level = new_level;
        log::set_max_level(new_level);
        self
    }

    fn output<W>(self, writer: W) -> Logger<W> {
        Logger {
            max_level: self.max_level,
            output: writer,
            lock: self.lock,
        }
    }
}

impl<T> Logger<T>
where
    T: Send + Sync + 'static,
    for<'a> &'a T: Write,
{
    fn init(self) -> Result<(), log::SetLoggerError> {
        log::set_max_level(self.max_level);
        log::set_boxed_logger(Box::new(self))
    }
}

const FAILED_WRITE_MSG: &str = "failed to write to log output";

impl<T> log::Log for Logger<T>
where
    T: Send + Sync + 'static,
    for<'a> &'a T: Write,
{
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let crate_root = module_path!().split("::").next().unwrap_or_default();
        let show_all = cfg!(debug_assertions);
        metadata.level() <= self.max_level
            && (show_all || metadata.target().starts_with(crate_root))
    }

    fn log(&self, record: &log::Record) {
        let _guard = self.lock.lock().unwrap();
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = humantime::format_rfc3339_millis(std::time::SystemTime::now());
        struct Printer<T>(T);
        use log::kv;
        impl<'kvs, T: Write> kv::VisitSource<'kvs> for Printer<T> {
            fn visit_pair(
                &mut self,
                key: kv::Key<'kvs>,
                value: kv::Value<'kvs>,
            ) -> Result<(), kv::Error> {
                write!(self.0, " {key}={value}").expect(FAILED_WRITE_MSG);
                Ok(())
            }
        }
        write!(
            &self.output,
            "{timestamp} {level} {module} {args}",
            timestamp = timestamp,
            level = record.level(),
            module = record.module_path().unwrap_or_default(),
            args = record.args(),
        )
        .expect(FAILED_WRITE_MSG);
        let _ = record.key_values().visit(&mut Printer(&self.output));
        (&self.output).write_all(b"\n").expect(FAILED_WRITE_MSG);
    }

    fn flush(&self) {
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }
}

#[derive(Debug)]
pub struct Error;

pub fn configure_logging() -> Result<(), Error> {
    let verbose = std::env::var("DEBUG")
        .map(|v| v.trim() == "1")
        .unwrap_or(false);

    let base_verbosity = if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Warn
    };
    let mut logger = Logger::new().level(base_verbosity.to_level_filter());
    if verbose {
        logger = logger.increase_level();
    }

    match std::env::var_os("LOGFILE") {
        Some(path) => {
            if path == "stderr" {
                logger.output(std::io::stderr()).init().map_err(|_| Error)
            } else if path == "stdout" {
                logger.output(std::io::stdout()).init().map_err(|_| Error)
            } else {
                let file = match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    Ok(file) => file,
                    Err(e) => {
                        eprintln!("Failed to open log file: {}", e);
                        return Err(Error);
                    }
                };
                logger.output(file).init().map_err(|_| Error)
            }
        }
        None => logger.output(std::io::stderr()).init().map_err(|_| Error),
    }
}
