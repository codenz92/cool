use std::collections::HashSet;
use std::env;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum ArgData {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Nil,
    List(Vec<ArgData>),
    Tuple(Vec<ArgData>),
    Dict(Vec<(ArgData, ArgData)>),
}

impl ArgData {
    pub fn type_name(&self) -> &'static str {
        match self {
            ArgData::Int(_) => "int",
            ArgData::Float(_) => "float",
            ArgData::Str(_) => "str",
            ArgData::Bool(_) => "bool",
            ArgData::Nil => "nil",
            ArgData::List(_) => "list",
            ArgData::Tuple(_) => "tuple",
            ArgData::Dict(_) => "dict",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ArgType {
    Str,
    Int,
    Float,
    Bool,
}

impl ArgType {
    fn from_spec(value: Option<ArgData>) -> Result<Self, String> {
        match value {
            None => Ok(Self::Str),
            Some(ArgData::Str(s)) => match s.as_str() {
                "str" | "string" => Ok(Self::Str),
                "int" => Ok(Self::Int),
                "float" => Ok(Self::Float),
                "bool" => Ok(Self::Bool),
                _ => Err(format!(
                    "argparse spec type must be one of str/int/float/bool, got '{}'",
                    s
                )),
            },
            Some(other) => Err(format!(
                "argparse spec type must be a string, got {}",
                other.type_name()
            )),
        }
    }

    fn takes_value(&self) -> bool {
        !matches!(self, Self::Bool)
    }
}

#[derive(Clone, Debug)]
struct PositionalSpec {
    name: String,
    help: Option<String>,
    arg_type: ArgType,
    required: bool,
    default: Option<ArgData>,
}

#[derive(Clone, Debug)]
struct OptionSpec {
    name: String,
    long_flag: String,
    short_flag: Option<String>,
    help: Option<String>,
    arg_type: ArgType,
    required: bool,
    default: Option<ArgData>,
}

#[derive(Clone, Debug)]
struct ParserSpec {
    prog: String,
    description: Option<String>,
    positionals: Vec<PositionalSpec>,
    options: Vec<OptionSpec>,
}

pub fn current_process_argv() -> Vec<String> {
    let mut argv = Vec::new();
    if let Ok(script_path) = env::var("COOL_SCRIPT_PATH") {
        argv.push(script_path);
    } else {
        argv.extend(env::args());
    }
    if let Ok(extra) = env::var("COOL_PROGRAM_ARGS") {
        if !extra.is_empty() {
            argv.extend(extra.split('\x1F').map(str::to_string));
        }
    }
    argv
}

pub fn default_prog_name() -> String {
    let argv0 = current_process_argv()
        .into_iter()
        .next()
        .unwrap_or_else(|| "program".to_string());
    Path::new(&argv0)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(argv0)
}

pub fn parse(spec: &ArgData, argv: &[String], fallback_prog: Option<&str>) -> Result<ArgData, String> {
    ParserSpec::from_argdata(spec, fallback_prog).and_then(|spec| spec.parse(argv))
}

pub fn help(spec: &ArgData, fallback_prog: Option<&str>) -> Result<String, String> {
    ParserSpec::from_argdata(spec, fallback_prog).map(|spec| spec.render_help())
}

impl ParserSpec {
    fn from_argdata(spec: &ArgData, fallback_prog: Option<&str>) -> Result<Self, String> {
        let spec_items = expect_dict(spec, "argparse spec must be a dict")?;
        let prog = match dict_get(&spec_items, "prog") {
            Some(ArgData::Str(s)) => s,
            Some(other) => {
                return Err(format!(
                    "argparse spec field 'prog' must be a string, got {}",
                    other.type_name()
                ))
            }
            None => fallback_prog.unwrap_or("program").to_string(),
        };
        let description = match dict_get(&spec_items, "description") {
            Some(ArgData::Str(s)) => Some(s),
            Some(ArgData::Nil) | None => None,
            Some(other) => {
                return Err(format!(
                    "argparse spec field 'description' must be a string, got {}",
                    other.type_name()
                ))
            }
        };

        let positional_values = match dict_get(&spec_items, "positionals") {
            Some(value) => expect_sequence(&value, "argparse spec field 'positionals' must be a list or tuple")?,
            None => Vec::new(),
        };
        let option_values = match dict_get(&spec_items, "options") {
            Some(value) => expect_sequence(&value, "argparse spec field 'options' must be a list or tuple")?,
            None => Vec::new(),
        };

        let mut positionals = Vec::with_capacity(positional_values.len());
        let mut seen_names = HashSet::new();
        for value in positional_values {
            let item = expect_dict(&value, "argparse positional entries must be dicts")?;
            let name = match dict_get(&item, "name") {
                Some(ArgData::Str(s)) if !s.is_empty() => s,
                Some(ArgData::Str(_)) => return Err("argparse positional name cannot be empty".into()),
                Some(other) => {
                    return Err(format!(
                        "argparse positional field 'name' must be a string, got {}",
                        other.type_name()
                    ))
                }
                None => return Err("argparse positional entries require a 'name' field".into()),
            };
            if !seen_names.insert(name.clone()) {
                return Err(format!("argparse positional name '{}' is duplicated", name));
            }
            let arg_type = ArgType::from_spec(dict_get(&item, "type"))?;
            let default = match dict_get(&item, "default") {
                Some(value) => Some(normalize_default_value(
                    &arg_type,
                    value,
                    &format!("argparse positional '{}'", name),
                )?),
                None => None,
            };
            let required = match dict_get(&item, "required") {
                Some(ArgData::Bool(b)) => b,
                Some(ArgData::Nil) | None => default.is_none(),
                Some(other) => {
                    return Err(format!(
                        "argparse positional '{}' field 'required' must be a bool, got {}",
                        name,
                        other.type_name()
                    ))
                }
            };
            let help = match dict_get(&item, "help") {
                Some(ArgData::Str(s)) => Some(s),
                Some(ArgData::Nil) | None => None,
                Some(other) => {
                    return Err(format!(
                        "argparse positional '{}' field 'help' must be a string, got {}",
                        name,
                        other.type_name()
                    ))
                }
            };
            positionals.push(PositionalSpec {
                name,
                help,
                arg_type,
                required,
                default,
            });
        }

        let mut options = Vec::with_capacity(option_values.len());
        let mut seen_option_names = HashSet::new();
        let mut seen_long_flags = HashSet::new();
        let mut seen_short_flags = HashSet::new();
        for value in option_values {
            let item = expect_dict(&value, "argparse option entries must be dicts")?;
            let name = match dict_get(&item, "name") {
                Some(ArgData::Str(s)) if !s.is_empty() => s,
                Some(ArgData::Str(_)) => return Err("argparse option name cannot be empty".into()),
                Some(other) => {
                    return Err(format!(
                        "argparse option field 'name' must be a string, got {}",
                        other.type_name()
                    ))
                }
                None => return Err("argparse option entries require a 'name' field".into()),
            };
            if !seen_option_names.insert(name.clone()) {
                return Err(format!("argparse option name '{}' is duplicated", name));
            }

            let arg_type = ArgType::from_spec(dict_get(&item, "type"))?;
            let default = match dict_get(&item, "default") {
                Some(value) => Some(normalize_default_value(
                    &arg_type,
                    value,
                    &format!("argparse option '{}'", name),
                )?),
                None => None,
            };
            let required = match dict_get(&item, "required") {
                Some(ArgData::Bool(b)) => b,
                Some(ArgData::Nil) | None => false,
                Some(other) => {
                    return Err(format!(
                        "argparse option '{}' field 'required' must be a bool, got {}",
                        name,
                        other.type_name()
                    ))
                }
            };
            let help = match dict_get(&item, "help") {
                Some(ArgData::Str(s)) => Some(s),
                Some(ArgData::Nil) | None => None,
                Some(other) => {
                    return Err(format!(
                        "argparse option '{}' field 'help' must be a string, got {}",
                        name,
                        other.type_name()
                    ))
                }
            };

            let long_flag = normalize_long_flag(match dict_get(&item, "long") {
                Some(ArgData::Str(s)) if !s.is_empty() => s,
                Some(ArgData::Str(_)) => {
                    return Err(format!("argparse option '{}' field 'long' cannot be empty", name))
                }
                Some(ArgData::Nil) | None => name.clone(),
                Some(other) => {
                    return Err(format!(
                        "argparse option '{}' field 'long' must be a string, got {}",
                        name,
                        other.type_name()
                    ))
                }
            });

            if !seen_long_flags.insert(long_flag.clone()) {
                return Err(format!("argparse option flag '{}' is duplicated", long_flag));
            }

            let short_flag = match dict_get(&item, "short") {
                Some(ArgData::Str(s)) if !s.is_empty() => {
                    let short = normalize_short_flag(&s)?;
                    if !seen_short_flags.insert(short.clone()) {
                        return Err(format!("argparse option flag '{}' is duplicated", short));
                    }
                    Some(short)
                }
                Some(ArgData::Str(_)) => {
                    return Err(format!("argparse option '{}' field 'short' cannot be empty", name))
                }
                Some(ArgData::Nil) | None => None,
                Some(other) => {
                    return Err(format!(
                        "argparse option '{}' field 'short' must be a string, got {}",
                        name,
                        other.type_name()
                    ))
                }
            };

            options.push(OptionSpec {
                name,
                long_flag,
                short_flag,
                help,
                arg_type,
                required,
                default,
            });
        }

        Ok(Self {
            prog,
            description,
            positionals,
            options,
        })
    }

    fn parse(&self, argv: &[String]) -> Result<ArgData, String> {
        let mut out: Vec<(ArgData, ArgData)> = Vec::new();
        for positional in &self.positionals {
            let value = positional.default.clone().unwrap_or(ArgData::Nil);
            set_dict_entry(&mut out, &positional.name, value);
        }
        for option in &self.options {
            let value = if let Some(default) = &option.default {
                default.clone()
            } else if matches!(option.arg_type, ArgType::Bool) {
                ArgData::Bool(false)
            } else {
                ArgData::Nil
            };
            set_dict_entry(&mut out, &option.name, value);
        }

        let mut seen_options = HashSet::new();
        let mut positional_tokens = Vec::new();
        let mut idx = 0;
        while idx < argv.len() {
            let token = &argv[idx];
            if token == "--" {
                positional_tokens.extend(argv[idx + 1..].iter().cloned());
                break;
            }
            if let Some(flag_body) = token.strip_prefix("--") {
                if flag_body.is_empty() {
                    positional_tokens.push(token.clone());
                    idx += 1;
                    continue;
                }
                let (flag_name, inline_value) = match flag_body.split_once('=') {
                    Some((flag, value)) => (format!("--{}", flag), Some(value.to_string())),
                    None => (format!("--{}", flag_body), None),
                };
                let Some(option) = self.options.iter().find(|opt| opt.long_flag == flag_name) else {
                    return Err(format!("argparse.parse(): unknown option '{}'", token));
                };
                let (value, consumed_next) = self.consume_option_value(option, inline_value, argv, idx)?;
                set_dict_entry(&mut out, &option.name, value);
                seen_options.insert(option.name.clone());
                idx += if consumed_next { 2 } else { 1 };
                continue;
            }
            if token.starts_with('-') && token.len() > 1 {
                let cluster = &token[1..];
                let chars: Vec<char> = cluster.chars().collect();
                let mut consumed_next = false;
                let mut cluster_idx = 0;
                while cluster_idx < chars.len() {
                    let short = format!("-{}", chars[cluster_idx]);
                    let Some(option) = self
                        .options
                        .iter()
                        .find(|opt| opt.short_flag.as_deref() == Some(short.as_str()))
                    else {
                        return Err(format!("argparse.parse(): unknown option '{}'", short));
                    };
                    let trailing = if cluster_idx + 1 < chars.len() {
                        Some(chars[cluster_idx + 1..].iter().collect::<String>())
                    } else {
                        None
                    };
                    let inline_value = if option.arg_type.takes_value() {
                        trailing.filter(|s| !s.is_empty())
                    } else {
                        None
                    };
                    let (value, used_next) = self.consume_option_value(option, inline_value, argv, idx)?;
                    set_dict_entry(&mut out, &option.name, value);
                    seen_options.insert(option.name.clone());
                    if option.arg_type.takes_value() {
                        consumed_next = used_next;
                        break;
                    }
                    cluster_idx += 1;
                }
                idx += if consumed_next { 2 } else { 1 };
                continue;
            }
            positional_tokens.push(token.clone());
            idx += 1;
        }

        if positional_tokens.len() > self.positionals.len() {
            return Err(format!(
                "argparse.parse(): unexpected positional argument '{}'",
                positional_tokens[self.positionals.len()]
            ));
        }

        for (positional, raw_value) in self.positionals.iter().zip(positional_tokens.iter()) {
            let value = convert_value(
                &positional.arg_type,
                raw_value,
                &format!("argparse positional '{}'", positional.name),
            )?;
            set_dict_entry(&mut out, &positional.name, value);
        }

        for positional in self.positionals.iter().skip(positional_tokens.len()) {
            if positional.required && positional.default.is_none() {
                return Err(format!(
                    "argparse.parse(): missing required positional '{}'",
                    positional.name
                ));
            }
        }

        for option in &self.options {
            if option.required && !seen_options.contains(&option.name) {
                return Err(format!(
                    "argparse.parse(): missing required option '{}'",
                    option.long_flag
                ));
            }
        }

        Ok(ArgData::Dict(out))
    }

    fn consume_option_value(
        &self,
        option: &OptionSpec,
        inline_value: Option<String>,
        argv: &[String],
        idx: usize,
    ) -> Result<(ArgData, bool), String> {
        if !option.arg_type.takes_value() {
            if let Some(value) = inline_value {
                return Ok((
                    convert_value(
                        &option.arg_type,
                        &value,
                        &format!("argparse option '{}'", option.long_flag),
                    )?,
                    false,
                ));
            }
            if let Some(next) = argv.get(idx + 1) {
                if let Some(value) = parse_optional_bool_literal(next) {
                    return Ok((ArgData::Bool(value), true));
                }
            }
            return Ok((ArgData::Bool(true), false));
        }

        if let Some(value) = inline_value {
            return Ok((
                convert_value(
                    &option.arg_type,
                    &value,
                    &format!("argparse option '{}'", option.long_flag),
                )?,
                false,
            ));
        }

        let Some(next) = argv.get(idx + 1) else {
            return Err(format!(
                "argparse.parse(): option '{}' requires a value",
                option.long_flag
            ));
        };

        Ok((
            convert_value(
                &option.arg_type,
                next,
                &format!("argparse option '{}'", option.long_flag),
            )?,
            true,
        ))
    }

    fn render_help(&self) -> String {
        let mut out = String::new();
        out.push_str("Usage: ");
        out.push_str(&self.prog);

        for option in &self.options {
            out.push(' ');
            if option.required {
                out.push_str(&option.usage_fragment());
            } else {
                out.push('[');
                out.push_str(&option.usage_fragment());
                out.push(']');
            }
        }
        for positional in &self.positionals {
            out.push(' ');
            if positional.required {
                out.push_str(&positional.metavar());
            } else {
                out.push('[');
                out.push_str(&positional.metavar());
                out.push(']');
            }
        }

        if let Some(description) = &self.description {
            out.push_str("\n\n");
            out.push_str(description);
        }

        if !self.positionals.is_empty() {
            out.push_str("\n\nPositional arguments:\n");
            for positional in &self.positionals {
                let mut suffixes = Vec::new();
                if positional.required {
                    suffixes.push("required".to_string());
                }
                if let Some(default) = &positional.default {
                    suffixes.push(format!("default: {}", render_value(default)));
                }
                out.push_str(&format_help_row(
                    &positional.metavar(),
                    positional.help.as_deref(),
                    &suffixes,
                ));
            }
        }

        if !self.options.is_empty() {
            out.push_str("\n\nOptions:\n");
            for option in &self.options {
                let mut suffixes = Vec::new();
                if option.required {
                    suffixes.push("required".to_string());
                }
                if let Some(default) = &option.default {
                    suffixes.push(format!("default: {}", render_value(default)));
                } else if matches!(option.arg_type, ArgType::Bool) {
                    suffixes.push("default: false".to_string());
                }
                out.push_str(&format_help_row(
                    &option.help_label(),
                    option.help.as_deref(),
                    &suffixes,
                ));
            }
        }

        out
    }
}

impl PositionalSpec {
    fn metavar(&self) -> String {
        self.name.to_uppercase()
    }
}

impl OptionSpec {
    fn metavar(&self) -> String {
        self.name.to_uppercase()
    }

    fn usage_fragment(&self) -> String {
        if self.arg_type.takes_value() {
            format!("{} {}", self.long_flag, self.metavar())
        } else {
            self.long_flag.clone()
        }
    }

    fn help_label(&self) -> String {
        let value = if self.arg_type.takes_value() {
            format!(" {}", self.metavar())
        } else {
            String::new()
        };
        match &self.short_flag {
            Some(short) => format!("{}, {}{}", short, self.long_flag, value),
            None => format!("{}{}", self.long_flag, value),
        }
    }
}

fn expect_dict(value: &ArgData, err: &str) -> Result<Vec<(ArgData, ArgData)>, String> {
    match value {
        ArgData::Dict(items) => Ok(items.clone()),
        _ => Err(err.to_string()),
    }
}

fn expect_sequence(value: &ArgData, err: &str) -> Result<Vec<ArgData>, String> {
    match value {
        ArgData::List(items) | ArgData::Tuple(items) => Ok(items.clone()),
        _ => Err(err.to_string()),
    }
}

fn dict_get(items: &[(ArgData, ArgData)], key: &str) -> Option<ArgData> {
    items.iter().find_map(|(k, v)| match k {
        ArgData::Str(s) if s == key => Some(v.clone()),
        _ => None,
    })
}

fn set_dict_entry(items: &mut Vec<(ArgData, ArgData)>, key: &str, value: ArgData) {
    let key_value = ArgData::Str(key.to_string());
    if let Some((_, slot)) = items.iter_mut().find(|(existing, _)| *existing == key_value) {
        *slot = value;
    } else {
        items.push((key_value, value));
    }
}

fn normalize_default_value(arg_type: &ArgType, value: ArgData, context: &str) -> Result<ArgData, String> {
    match arg_type {
        ArgType::Str => match value {
            ArgData::Str(_) | ArgData::Nil => Ok(value),
            other => Err(format!(
                "{} default must be a string or nil, got {}",
                context,
                other.type_name()
            )),
        },
        ArgType::Int => match value {
            ArgData::Int(_) | ArgData::Nil => Ok(value),
            other => Err(format!(
                "{} default must be an int or nil, got {}",
                context,
                other.type_name()
            )),
        },
        ArgType::Float => match value {
            ArgData::Float(_) | ArgData::Nil => Ok(value),
            ArgData::Int(n) => Ok(ArgData::Float(n as f64)),
            other => Err(format!(
                "{} default must be a float/int or nil, got {}",
                context,
                other.type_name()
            )),
        },
        ArgType::Bool => match value {
            ArgData::Bool(_) => Ok(value),
            other => Err(format!("{} default must be a bool, got {}", context, other.type_name())),
        },
    }
}

fn convert_value(arg_type: &ArgType, raw: &str, context: &str) -> Result<ArgData, String> {
    match arg_type {
        ArgType::Str => Ok(ArgData::Str(raw.to_string())),
        ArgType::Int => raw
            .parse::<i64>()
            .map(ArgData::Int)
            .map_err(|_| format!("{} expects an int, got '{}'", context, raw)),
        ArgType::Float => raw
            .parse::<f64>()
            .map(ArgData::Float)
            .map_err(|_| format!("{} expects a float, got '{}'", context, raw)),
        ArgType::Bool => parse_optional_bool_literal(raw)
            .map(ArgData::Bool)
            .ok_or_else(|| format!("{} expects a bool, got '{}'", context, raw)),
    }
}

fn parse_optional_bool_literal(raw: &str) -> Option<bool> {
    match raw {
        "1" | "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => Some(true),
        "0" | "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => Some(false),
        _ => None,
    }
}

fn normalize_long_flag(value: String) -> String {
    if value.starts_with("--") {
        value
    } else {
        format!("--{}", value.replace('_', "-"))
    }
}

fn normalize_short_flag(value: &str) -> Result<String, String> {
    let rendered = if value.starts_with('-') {
        value.to_string()
    } else {
        format!("-{}", value)
    };
    if !rendered.starts_with('-') || rendered.starts_with("--") || rendered.chars().count() != 2 {
        return Err(format!(
            "argparse short option '{}' must be a single-character flag",
            value
        ));
    }
    Ok(rendered)
}

fn format_help_row(label: &str, help: Option<&str>, suffixes: &[String]) -> String {
    let mut text = help.unwrap_or("").to_string();
    if !suffixes.is_empty() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push('(');
        text.push_str(&suffixes.join(", "));
        text.push(')');
    }
    format!("  {:<24} {}\n", label, text)
}

fn render_value(value: &ArgData) -> String {
    match value {
        ArgData::Int(n) => n.to_string(),
        ArgData::Float(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{:.1}", n)
            } else {
                n.to_string()
            }
        }
        ArgData::Str(s) => s.clone(),
        ArgData::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        ArgData::Nil => "nil".to_string(),
        ArgData::List(items) => {
            let rendered: Vec<String> = items.iter().map(render_value).collect();
            format!("[{}]", rendered.join(", "))
        }
        ArgData::Tuple(items) => {
            let rendered: Vec<String> = items.iter().map(render_value).collect();
            if items.len() == 1 {
                format!("({},)", rendered[0])
            } else {
                format!("({})", rendered.join(", "))
            }
        }
        ArgData::Dict(items) => {
            let rendered: Vec<String> = items
                .iter()
                .map(|(k, v)| format!("{}: {}", render_value(k), render_value(v)))
                .collect();
            format!("{{{}}}", rendered.join(", "))
        }
    }
}
