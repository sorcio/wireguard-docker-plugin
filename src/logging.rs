struct Logger {
    max_level: log::LevelFilter,
}

impl Logger {
    const fn new() -> Self {
        Self {
            max_level: log::LevelFilter::Off,
        }
    }

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

    fn init(self) -> Result<(), log::SetLoggerError> {
        log::set_max_level(self.max_level);
        log::set_boxed_logger(Box::new(self))
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.max_level && metadata.target().starts_with(module_path!())
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = humantime::format_rfc3339_millis(std::time::SystemTime::now());
        struct Printer;
        use log::kv;
        impl<'kvs> kv::VisitSource<'kvs> for Printer {
            fn visit_pair(
                &mut self,
                key: kv::Key<'kvs>,
                value: kv::Value<'kvs>,
            ) -> Result<(), kv::Error> {
                eprint!(" {key}={value}");
                Ok(())
            }
        }
        eprint!(
            "{timestamp} {level} {args}",
            timestamp = timestamp,
            level = record.level(),
            args = record.args(),
        );
        let _ = record.key_values().visit(&mut Printer);
        eprintln!();
    }

    fn flush(&self) {
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }
}

pub(crate) fn configure_logging() {
    // TODO: fix this when we have a proper argument parser
    let verbose = std::env::args().find(|i| i == "-v").is_some();
    let base_verbosity = if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Warn
    };
    let mut logger = Logger::new().level(base_verbosity.to_level_filter());
    if verbose {
        logger = logger.increase_level();
    }
    let _ = logger.init();
}
