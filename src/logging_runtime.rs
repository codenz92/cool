use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq)]
pub enum LogData {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Nil,
    List(Vec<LogData>),
    Tuple(Vec<LogData>),
    Dict(Vec<(LogData, LogData)>),
}

impl LogData {
    pub fn type_name(&self) -> &'static str {
        match self {
            LogData::Int(_) => "int",
            LogData::Float(_) => "float",
            LogData::Str(_) => "str",
            LogData::Bool(_) => "bool",
            LogData::Nil => "nil",
            LogData::List(_) => "list",
            LogData::Tuple(_) => "tuple",
            LogData::Dict(_) => "dict",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warning" | "warn" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            _ => Err(format!(
                "logging level must be one of DEBUG/INFO/WARNING/ERROR, got '{}'",
                raw
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoggingConfig {
    pub level: LogLevel,
    pub format: Option<String>,
    pub timestamp: bool,
    pub stdout: bool,
    pub file: Option<String>,
    pub append: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            format: None,
            timestamp: false,
            stdout: true,
            file: None,
            append: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoggingState {
    pub config: LoggingConfig,
    file_needs_reset: bool,
}

impl Default for LoggingState {
    fn default() -> Self {
        Self {
            config: LoggingConfig::default(),
            file_needs_reset: false,
        }
    }
}

pub fn configure(state: &mut LoggingState, value: Option<&LogData>) -> Result<(), String> {
    let mut config = LoggingConfig::default();
    if let Some(value) = value {
        match value {
            LogData::Nil => {}
            LogData::Dict(items) => {
                for (raw_key, raw_value) in items {
                    let key = match raw_key {
                        LogData::Str(s) => s.as_str(),
                        other => {
                            return Err(format!(
                                "logging.basic_config() keys must be strings, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    match key {
                        "level" => match raw_value {
                            LogData::Str(s) => config.level = LogLevel::parse(s)?,
                            other => {
                                return Err(format!(
                                    "logging.basic_config() field 'level' must be a string, got {}",
                                    other.type_name()
                                ))
                            }
                        },
                        "format" => match raw_value {
                            LogData::Str(s) => config.format = Some(s.clone()),
                            LogData::Nil => config.format = None,
                            other => {
                                return Err(format!(
                                    "logging.basic_config() field 'format' must be a string or nil, got {}",
                                    other.type_name()
                                ))
                            }
                        },
                        "timestamp" => match raw_value {
                            LogData::Bool(b) => config.timestamp = *b,
                            LogData::Nil => config.timestamp = false,
                            other => {
                                return Err(format!(
                                    "logging.basic_config() field 'timestamp' must be a bool, got {}",
                                    other.type_name()
                                ))
                            }
                        },
                        "stdout" => match raw_value {
                            LogData::Bool(b) => config.stdout = *b,
                            LogData::Nil => config.stdout = false,
                            other => {
                                return Err(format!(
                                    "logging.basic_config() field 'stdout' must be a bool, got {}",
                                    other.type_name()
                                ))
                            }
                        },
                        "file" => match raw_value {
                            LogData::Str(s) => {
                                if s.is_empty() {
                                    return Err("logging.basic_config() field 'file' cannot be empty".into());
                                }
                                config.file = Some(s.clone());
                            }
                            LogData::Nil => config.file = None,
                            other => {
                                return Err(format!(
                                    "logging.basic_config() field 'file' must be a string or nil, got {}",
                                    other.type_name()
                                ))
                            }
                        },
                        "append" => match raw_value {
                            LogData::Bool(b) => config.append = *b,
                            LogData::Nil => config.append = false,
                            other => {
                                return Err(format!(
                                    "logging.basic_config() field 'append' must be a bool, got {}",
                                    other.type_name()
                                ))
                            }
                        },
                        other => return Err(format!("logging.basic_config() does not support field '{}'", other)),
                    }
                }
            }
            other => {
                return Err(format!(
                    "logging.basic_config() expects a config dict, got {}",
                    other.type_name()
                ))
            }
        }
    }

    state.file_needs_reset = config.file.is_some() && !config.append;
    state.config = config;
    Ok(())
}

pub fn emit(state: &mut LoggingState, level: LogLevel, message: &str, name: Option<&str>) -> Result<(), String> {
    if level < state.config.level {
        return Ok(());
    }

    let line = format_line(&state.config, level, message, name);

    if state.config.stdout {
        println!("{}", line);
        std::io::stdout().flush().ok();
    }

    if let Some(path) = &state.config.file {
        let mut opts = OpenOptions::new();
        opts.create(true).write(true);
        if state.file_needs_reset {
            opts.truncate(true);
        } else {
            opts.append(true);
        }
        let mut file = opts
            .open(path)
            .map_err(|e| format!("logging file error for '{}': {}", path, e))?;
        writeln!(file, "{}", line).map_err(|e| format!("logging file error for '{}': {}", path, e))?;
        state.file_needs_reset = false;
    }

    Ok(())
}

fn format_line(config: &LoggingConfig, level: LogLevel, message: &str, name: Option<&str>) -> String {
    let logger_name = name.unwrap_or("");
    let needs_timestamp = config.timestamp
        || config
            .format
            .as_deref()
            .map(|fmt| fmt.contains("{timestamp}"))
            .unwrap_or(false);
    let timestamp = if needs_timestamp {
        current_timestamp()
    } else {
        String::new()
    };

    if let Some(fmt) = &config.format {
        return fmt
            .replace("{timestamp}", &timestamp)
            .replace("{level}", level.as_str())
            .replace("{name}", logger_name)
            .replace("{message}", message);
    }

    let mut out = String::new();
    if config.timestamp {
        out.push_str(&timestamp);
        out.push(' ');
    }
    out.push('[');
    out.push_str(level.as_str());
    out.push(']');
    out.push(' ');
    if !logger_name.is_empty() {
        out.push_str(logger_name);
        out.push_str(": ");
    }
    out.push_str(message);
    out
}

fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
