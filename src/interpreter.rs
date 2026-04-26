use crate::argparse_runtime::{self, ArgData};
use crate::ast::*;
use crate::core_runtime;
use crate::csv_runtime;
use crate::datetime_runtime::{self, DateTimeParts};
use crate::hashlib_runtime;
use crate::http_runtime;
use crate::logging_runtime::{self, LogData, LogLevel};
use crate::project::ModuleResolver;
use crate::sqlite_runtime::{self, SqlData};
use crate::subprocess_runtime::{run_shell_command, SubprocessResult};
use crate::toml_runtime::{self, TomlData};
use crate::yaml_runtime::{self, YamlData};
/// Tree-walk interpreter for Cool.
use crossterm::event::{self as ct_event, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

// ── Readline / Tab-completion ─────────────────────────────────────────────────

thread_local! {
    static COMPLETIONS: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

/// A rustyline helper that completes against the `COMPLETIONS` list.
struct CoolHelper;

impl rustyline::completion::Completer for CoolHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        let word_start = line[..pos].rfind(|c: char| c == ' ' || c == '\t').map_or(0, |i| i + 1);
        let word = &line[word_start..pos];
        let candidates: Vec<String> =
            COMPLETIONS.with(|c| c.borrow().iter().filter(|s| s.starts_with(word)).cloned().collect());
        Ok((word_start, candidates))
    }
}

impl rustyline::hint::Hinter for CoolHelper {
    type Hint = String;
}
impl rustyline::highlight::Highlighter for CoolHelper {}
impl rustyline::validate::Validator for CoolHelper {}
impl rustyline::Helper for CoolHelper {}

// ── Environment ───────────────────────────────────────────────────────────────

/// A ref-counted scope. Cheap to clone (just bumps the Rc count).
#[derive(Debug, Clone)]
pub struct Env(Rc<RefCell<EnvData>>);

#[derive(Debug)]
struct EnvData {
    vars: HashMap<String, Value>,
    parent: Option<Env>,
    /// Names declared `global` — assignments go to the root scope
    globals: std::collections::HashSet<String>,
    /// Names declared `nonlocal` — assignments skip current scope and go up
    nonlocals: std::collections::HashSet<String>,
}

impl Env {
    pub fn new_global() -> Self {
        let mut data = EnvData {
            vars: HashMap::new(),
            parent: None,
            globals: Default::default(),
            nonlocals: Default::default(),
        };
        for name in &[
            "print",
            "len",
            "range",
            "str",
            "int",
            "i8",
            "u8",
            "i16",
            "u16",
            "i32",
            "u32",
            "i64",
            "isize",
            "usize",
            "word_bits",
            "word_bytes",
            "float",
            "bool",
            "type",
            "input",
            "append",
            "pop",
            "keys",
            "values",
            "items",
            "sorted",
            "reversed",
            "enumerate",
            "zip",
            "abs",
            "round",
            "min",
            "max",
            "sum",
            "any",
            "all",
            "map",
            "filter",
            "repr",
            "exit",
            "open",
            "isinstance",
            "hasattr",
            "getattr",
            "runfile",
            "super",
            "list",
            "tuple",
            "dict",
            "set",
            "set_completions",
            "eval",
            // LLVM-only builtins (give a helpful error in the interpreter)
            "asm",
            "malloc",
            "free",
            "read_byte",
            "write_byte",
            "read_i8",
            "write_i8",
            "read_u8",
            "write_u8",
            "read_i16",
            "write_i16",
            "read_u16",
            "write_u16",
            "read_i32",
            "write_i32",
            "read_u32",
            "write_u32",
            "read_i64",
            "write_i64",
            "read_f64",
            "write_f64",
            "read_byte_volatile",
            "write_byte_volatile",
            "read_i8_volatile",
            "write_i8_volatile",
            "read_u8_volatile",
            "write_u8_volatile",
            "read_i16_volatile",
            "write_i16_volatile",
            "read_u16_volatile",
            "write_u16_volatile",
            "read_i32_volatile",
            "write_i32_volatile",
            "read_u32_volatile",
            "write_u32_volatile",
            "read_i64_volatile",
            "write_i64_volatile",
            "read_f64_volatile",
            "write_f64_volatile",
            "read_str",
            "write_str",
            "outb",
            "inb",
            "write_serial_byte",
        ] {
            data.vars.insert(name.to_string(), Value::BuiltinFn(name.to_string()));
        }
        Env(Rc::new(RefCell::new(data)))
    }

    fn new_child(parent: Env) -> Self {
        Env(Rc::new(RefCell::new(EnvData {
            vars: HashMap::new(),
            parent: Some(parent),
            globals: Default::default(),
            nonlocals: Default::default(),
        })))
    }

    fn get(&self, name: &str) -> Option<Value> {
        let data = self.0.borrow();
        if let Some(v) = data.vars.get(name) {
            return Some(v.clone());
        }
        if let Some(p) = &data.parent {
            return p.get(name);
        }
        None
    }

    fn set_local(&self, name: String, value: Value) {
        // Read flags without holding borrow across calls
        let is_global = self.0.borrow().globals.contains(&name);
        let is_nonlocal = self.0.borrow().nonlocals.contains(&name);
        let parent = self.0.borrow().parent.clone();
        if is_global {
            self.root().0.borrow_mut().vars.insert(name, value);
        } else if is_nonlocal {
            if let Some(p) = parent {
                p.assign(&name, value);
            } else {
                self.0.borrow_mut().vars.insert(name, value);
            }
        } else {
            self.0.borrow_mut().vars.insert(name, value);
        }
    }

    fn assign(&self, name: &str, value: Value) {
        let is_global = self.0.borrow().globals.contains(name);
        let is_nonlocal = self.0.borrow().nonlocals.contains(name);
        let has_var = self.0.borrow().vars.contains_key(name);
        let parent = self.0.borrow().parent.clone();
        if is_global {
            self.root().0.borrow_mut().vars.insert(name.to_string(), value);
        } else if is_nonlocal {
            if let Some(p) = parent {
                p.assign(name, value);
            } else {
                self.0.borrow_mut().vars.insert(name.to_string(), value);
            }
        } else if has_var {
            self.0.borrow_mut().vars.insert(name.to_string(), value);
        } else if let Some(p) = parent {
            p.assign(name, value);
        } else {
            self.0.borrow_mut().vars.insert(name.to_string(), value);
        }
    }

    fn declare_global(&self, name: String) {
        self.0.borrow_mut().globals.insert(name);
    }

    fn declare_nonlocal(&self, name: String) {
        self.0.borrow_mut().nonlocals.insert(name);
    }

    fn root(&self) -> Env {
        let data = self.0.borrow();
        match &data.parent {
            None => self.clone(),
            Some(p) => {
                let p = p.clone();
                drop(data);
                p.root()
            }
        }
    }
}

// ── Values ────────────────────────────────────────────────────────────────────

/// A Cool class definition.
#[derive(Debug)]
pub struct CoolClass {
    pub name: String,
    pub parent: Option<Rc<CoolClass>>,
    pub methods: HashMap<String, Value>,
}

/// A Cool object instance.
#[derive(Debug)]
pub struct CoolInstance {
    pub class: Rc<CoolClass>,
    pub fields: RefCell<HashMap<String, Value>>,
}

/// An open file handle.
#[derive(Debug)]
pub struct FileHandle {
    pub path: String,
    pub mode: String,
    pub content: Vec<String>, // lines for "r"/"r+" modes
    pub line_pos: usize,      // next line to read
    pub write_buf: RefCell<String>,
    pub closed: bool,
}

// ── Socket handle ─────────────────────────────────────────────────────────────

pub enum SocketKind {
    Stream(std::net::TcpStream),
    Listener(std::net::TcpListener),
}

impl std::fmt::Debug for SocketKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocketKind::Stream(s) => write!(f, "Stream({:?})", s),
            SocketKind::Listener(l) => write!(f, "Listener({:?})", l),
        }
    }
}

#[derive(Debug)]
pub struct SocketHandle {
    pub kind: SocketKind,
    pub closed: bool,
    pub peer: String,
}

// ── FFI handle ────────────────────────────────────────────────────────────────

/// Handle to a dynamically-loaded shared library.
#[derive(Clone)]
pub struct FfiLibHandle(pub std::sync::Arc<libloading::Library>);

impl std::fmt::Debug for FfiLibHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<ffi library>")
    }
}

// ── Values ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Nil,
    List(Rc<RefCell<Vec<Value>>>),
    Dict(Rc<RefCell<IndexedMap>>),
    Tuple(Rc<Vec<Value>>),
    Function {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
        closure: Env,
    },
    BuiltinFn(String),
    Class(Rc<CoolClass>),
    Instance(Rc<CoolInstance>),
    File(Rc<RefCell<FileHandle>>),
    Socket(Rc<RefCell<SocketHandle>>),
    /// super() proxy: holds the instance and the parent class to dispatch on
    Super {
        instance: Rc<CoolInstance>,
        parent: Rc<CoolClass>,
    },
    /// A loaded shared library (from ffi.open)
    FfiLib(FfiLibHandle),
    /// A callable C function with resolved symbol address and type info
    FfiFunc {
        #[allow(dead_code)]
        lib: FfiLibHandle,
        sym: usize,
        name: String,
        ret_type: String,
        arg_types: Vec<String>,
    },
}

/// An ordered key-value store (preserves insertion order like Python dicts).
#[derive(Debug, Clone)]
pub struct IndexedMap {
    pub keys: Vec<Value>,
    pub vals: Vec<Value>,
}

impl IndexedMap {
    fn new() -> Self {
        IndexedMap {
            keys: Vec::new(),
            vals: Vec::new(),
        }
    }

    fn get(&self, key: &Value) -> Option<Value> {
        self.keys
            .iter()
            .position(|k| values_equal(k, key))
            .map(|i| self.vals[i].clone())
    }

    fn set(&mut self, key: Value, val: Value) {
        if let Some(i) = self.keys.iter().position(|k| values_equal(k, &key)) {
            self.vals[i] = val;
        } else {
            self.keys.push(key);
            self.vals.push(val);
        }
    }

    fn contains(&self, key: &Value) -> bool {
        self.keys.iter().any(|k| values_equal(k, key))
    }

    fn remove(&mut self, key: &Value) -> Option<Value> {
        if let Some(i) = self.keys.iter().position(|k| values_equal(k, key)) {
            self.keys.remove(i);
            Some(self.vals.remove(i))
        } else {
            None
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&Value, &Value)> {
        self.keys.iter().zip(self.vals.iter())
    }
}

// ── Display ───────────────────────────────────────────────────────────────────

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Str(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            Value::Nil => write!(f, "nil"),
            Value::List(v) => {
                let v = v.borrow();
                write!(f, "[")?;
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", repr(item))?;
                }
                write!(f, "]")
            }
            Value::Dict(m) => {
                let m = m.borrow();
                write!(f, "{{")?;
                for (i, (k, v)) in m.keys.iter().zip(m.vals.iter()).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", repr(k), repr(v))?;
                }
                write!(f, "}}")
            }
            Value::Tuple(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", repr(item))?;
                }
                if items.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            Value::Function { name, .. } => write!(f, "<function {}>", name),
            Value::BuiltinFn(name) => write!(f, "<builtin {}>", name),
            Value::Class(cls) => write!(f, "<class {}>", cls.name),
            Value::Instance(inst) => write!(f, "<{} object>", inst.class.name),
            Value::File(fh) => write!(f, "<file '{}'>", fh.borrow().path),
            Value::Socket(sh) => write!(f, "<socket '{}'>", sh.borrow().peer),
            Value::Super { parent, .. } => write!(f, "<super of {}>", parent.name),
            Value::FfiLib(_) => write!(f, "<ffi library>"),
            Value::FfiFunc {
                name,
                ret_type,
                arg_types,
                ..
            } => write!(f, "<ffi func {}({}) -> {}>", name, arg_types.join(", "), ret_type),
        }
    }
}

fn repr(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        other => other.to_string(),
    }
}

impl Value {
    fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(false) | Value::Nil => false,
            Value::Int(0) => false,
            Value::Str(s) if s.is_empty() => false,
            Value::List(v) => !v.borrow().is_empty(),
            Value::Dict(m) => !m.borrow().keys.is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::FfiLib(_) | Value::FfiFunc { .. } => true,
            _ => true,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Bool(_) => "bool",
            Value::Nil => "nil",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Tuple(_) => "tuple",
            Value::Function { .. } => "function",
            Value::BuiltinFn(_) => "builtin",
            Value::Class(_) => "class",
            Value::Instance(_) => "instance",
            Value::File(_) => "file",
            Value::Socket(_) => "socket",
            Value::Super { .. } => "super",
            Value::FfiLib(_) => "ffi_lib",
            Value::FfiFunc { .. } => "ffi_func",
        }
    }
}

// ── Control flow signals ──────────────────────────────────────────────────────

enum Signal {
    None,
    Return(Value),
    Break,
    Continue,
    Raise(Value),
}

// ── Interpreter ───────────────────────────────────────────────────────────────

type RlEditor = rustyline::Editor<CoolHelper, rustyline::history::DefaultHistory>;

pub struct Interpreter {
    pub current_line: usize,
    pub source_dir: std::path::PathBuf,
    module_resolver: ModuleResolver,
    /// Source lines, for rich error messages.
    source_lines: Vec<String>,
    /// Stash for a raised exception value that must cross a Result<Value,String> boundary.
    /// Set in call_value when a user function raises; cleared when try/except catches it.
    pub pending_raise: Option<Value>,
    /// Rustyline editor — Some when running interactively, None as fallback.
    readline_editor: Option<RlEditor>,
    /// xorshift64 state for the random module.
    rng_state: u64,
    logging_state: logging_runtime::LoggingState,
}

macro_rules! numeric_op {
    ($self:expr, $l:expr, $r:expr, $op:tt, $name:expr) => {
        match ($l, $r) {
            (Value::Int(a),   Value::Int(b))   => Ok(Value::Int(a $op b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a $op b)),
            (Value::Int(a),   Value::Float(b)) => Ok(Value::Float((a as f64) $op b)),
            (Value::Float(a), Value::Int(b))   => Ok(Value::Float(a $op (b as f64))),
            (l, r) => Err($self.err(&format!("cannot {} {} and {}", $name, l.type_name(), r.type_name()))),
        }
    };
}

macro_rules! compare_op {
    ($self:expr, $l:expr, $r:expr, $op:tt) => {
        match ($l, $r) {
            (Value::Int(a),   Value::Int(b))   => Ok(Value::Bool(a $op b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a $op b)),
            (Value::Int(a),   Value::Float(b)) => Ok(Value::Bool((a as f64) $op b)),
            (Value::Float(a), Value::Int(b))   => Ok(Value::Bool(a $op (b as f64))),
            (Value::Str(a),   Value::Str(b))   => Ok(Value::Bool(a $op b)),
            (l, r) => Err($self.err(&format!("cannot compare {} and {}", l.type_name(), r.type_name()))),
        }
    };
}

impl Interpreter {
    pub fn new(source_dir: std::path::PathBuf, source: &str, module_resolver: ModuleResolver) -> Self {
        let readline_editor = rustyline::Editor::<CoolHelper, rustyline::history::DefaultHistory>::new()
            .ok()
            .map(|mut ed| {
                ed.set_helper(Some(CoolHelper));
                ed
            });
        // Seed xorshift64 from system time; fall back to a fixed seed if time is unavailable.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(12345678901234567);
        let rng_state = if seed == 0 { 1 } else { seed };
        Interpreter {
            current_line: 0,
            source_dir,
            module_resolver,
            source_lines: source.lines().map(|l| l.to_string()).collect(),
            pending_raise: None,
            readline_editor,
            rng_state,
            logging_state: logging_runtime::LoggingState::default(),
        }
    }

    fn err(&self, msg: &str) -> String {
        let snippet = if self.current_line > 0 {
            self.source_lines
                .get(self.current_line.saturating_sub(1))
                .map(|line| {
                    let trimmed = line.trim_start();
                    let indent = line.len() - trimmed.len();
                    format!("\n    {}\n    {}^", line, " ".repeat(indent))
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        format!("line {}: {}{}", self.current_line, msg, snippet)
    }

    pub fn run(&mut self, program: &Program) -> Result<(), String> {
        let env = Env::new_global();
        let result = match self.exec_block(program, &env) {
            Err(e) if e == "__raise__" => {
                let v = self.pending_raise.take().unwrap_or(Value::Nil);
                Ok(Signal::Raise(v))
            }
            other => other,
        }?;
        match result {
            Signal::Return(v) => eprintln!("warning: top-level return of {}", v),
            Signal::Break => return Err("line {}: 'break' outside loop".to_string()),
            Signal::Continue => return Err("line {}: 'continue' outside loop".to_string()),
            Signal::Raise(v) => return Err(format!("Unhandled exception: {}", v)),
            Signal::None => {}
        }
        Ok(())
    }

    fn exec_block(&mut self, stmts: &[Stmt], env: &Env) -> Result<Signal, String> {
        for stmt in stmts {
            match self.exec_stmt(stmt, env)? {
                Signal::None => {}
                other => return Ok(other),
            }
        }
        Ok(Signal::None)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: &Env) -> Result<Signal, String> {
        match stmt {
            Stmt::SetLine(n) => {
                self.current_line = *n;
                Ok(Signal::None)
            }
            Stmt::Pass => Ok(Signal::None),
            Stmt::Break => Ok(Signal::Break),
            Stmt::Continue => Ok(Signal::Continue),

            Stmt::Expr(expr) => {
                self.eval(expr, env)?;
                Ok(Signal::None)
            }

            Stmt::Assign { name, value } => {
                let v = self.eval(value, env)?;
                env.set_local(name.clone(), v);
                Ok(Signal::None)
            }

            Stmt::Unpack { names, value } => {
                let v = self.eval(value, env)?;
                let items = self.to_iterable(v)?;
                if items.len() != names.len() {
                    return Err(self.err(&format!("unpack: expected {} values, got {}", names.len(), items.len())));
                }
                for (name, val) in names.iter().zip(items) {
                    env.set_local(name.clone(), val);
                }
                Ok(Signal::None)
            }

            Stmt::UnpackTargets { targets, value } => {
                let v = self.eval(value, env)?;
                let items = self.to_iterable(v)?;
                if items.len() != targets.len() {
                    return Err(self.err(&format!(
                        "unpack: expected {} values, got {}",
                        targets.len(),
                        items.len()
                    )));
                }
                for (target, val) in targets.iter().zip(items) {
                    match target {
                        Expr::Ident(name) => {
                            env.set_local(name.clone(), val);
                        }
                        Expr::Index { object, index } => {
                            let obj = self.eval(object, env)?;
                            let idx = self.eval(index, env)?;
                            match obj {
                                Value::List(lst) => {
                                    let i = to_list_index(&lst.borrow(), idx, self.current_line)?;
                                    lst.borrow_mut()[i] = val;
                                }
                                Value::Dict(map) => {
                                    map.borrow_mut().set(idx, val);
                                }
                                other => return Err(self.err(&format!("cannot index-assign on {}", other.type_name()))),
                            }
                        }
                        Expr::Attr { object, name } => {
                            let obj = self.eval(object, env)?;
                            match obj {
                                Value::Dict(map) => {
                                    map.borrow_mut().set(Value::Str(name.clone()), val);
                                }
                                Value::Instance(inst) => {
                                    inst.fields.borrow_mut().insert(name.clone(), val);
                                }
                                other => return Err(self.err(&format!("cannot set attr on {}", other.type_name()))),
                            }
                        }
                        _ => return Err(self.err("invalid unpack target")),
                    }
                }
                Ok(Signal::None)
            }

            Stmt::AugAssign { name, op, value } => {
                let cur = env
                    .get(name)
                    .ok_or_else(|| self.err(&format!("undefined variable '{}'", name)))?;
                let rhs = self.eval(value, env)?;
                let result = self.apply_binop(op, cur, rhs)?;
                env.assign(name, result);
                Ok(Signal::None)
            }

            Stmt::SetItem { object, index, value } => {
                let obj = self.eval(object, env)?;
                let idx = self.eval(index, env)?;
                let val = self.eval(value, env)?;
                match obj {
                    Value::List(lst) => {
                        let i = to_list_index(&lst.borrow(), idx, self.current_line)?;
                        lst.borrow_mut()[i] = val;
                    }
                    Value::Dict(map) => {
                        map.borrow_mut().set(idx, val);
                    }
                    other => {
                        return Err(self.err(&format!("cannot index-assign on {}", other.type_name())));
                    }
                }
                Ok(Signal::None)
            }

            Stmt::SetAttr { object, name, value } => {
                let obj = self.eval(object, env)?;
                let val = self.eval(value, env)?;
                match obj {
                    Value::Dict(map) => {
                        map.borrow_mut().set(Value::Str(name.clone()), val);
                        Ok(Signal::None)
                    }
                    Value::Instance(inst) => {
                        inst.fields.borrow_mut().insert(name.clone(), val);
                        Ok(Signal::None)
                    }
                    other => Err(self.err(&format!("cannot set attribute on {}", other.type_name()))),
                }
            }

            Stmt::Return(expr) => {
                let v = match expr {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Nil,
                };
                Ok(Signal::Return(v))
            }

            Stmt::Raise(expr) => {
                let v = match expr {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Str("Exception".to_string()),
                };
                Ok(Signal::Raise(v))
            }

            Stmt::FnDef { name, params, body, .. } => {
                let func = Value::Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    closure: env.clone(),
                };
                env.set_local(name.clone(), func);
                Ok(Signal::None)
            }

            Stmt::ExternFn { .. } => {
                Err(self.err("extern declarations are only supported in the LLVM backend — compile with `cool build`"))
            }

            Stmt::Data { .. } => {
                Err(self.err("data declarations are only supported in the LLVM backend — compile with `cool build`"))
            }

            Stmt::Class { name, parent, body } => {
                let parent_class = if let Some(pname) = parent {
                    match env.get(pname) {
                        Some(Value::Class(cls)) => Some(cls),
                        Some(_) => return Err(self.err(&format!("'{}' is not a class", pname))),
                        None => return Err(self.err(&format!("undefined class '{}'", pname))),
                    }
                } else {
                    None
                };

                // Execute the class body to collect methods
                let class_env = Env::new_child(env.clone());
                self.exec_block(body, &class_env)?;
                let mut methods = HashMap::new();
                for (k, v) in &class_env.0.borrow().vars {
                    methods.insert(k.clone(), v.clone());
                }

                let cls = Rc::new(CoolClass {
                    name: name.clone(),
                    parent: parent_class,
                    methods,
                });
                env.set_local(name.clone(), Value::Class(cls));
                Ok(Signal::None)
            }

            Stmt::Struct { name, fields, .. } => {
                // Lower struct to a class with a typed-field __init__.
                let mut init_body = Vec::new();
                let mut params = vec![crate::ast::Param {
                    name: "self".to_string(),
                    default: None,
                    is_vararg: false,
                    is_kwarg: false,
                    type_name: None,
                }];
                for (field_name, type_name) in fields {
                    params.push(crate::ast::Param {
                        name: field_name.clone(),
                        default: None,
                        is_vararg: false,
                        is_kwarg: false,
                        type_name: None,
                    });
                    // self.field = type_fn(field) — coerce on construction
                    let coerce_name = match type_name.as_str() {
                        "f32" | "f64" => "float",
                        other => other,
                    };
                    let coerce_expr = if matches!(
                        type_name.as_str(),
                        "i8" | "u8"
                            | "i16"
                            | "u16"
                            | "i32"
                            | "u32"
                            | "i64"
                            | "u64"
                            | "isize"
                            | "usize"
                            | "f32"
                            | "f64"
                            | "float"
                            | "bool"
                    ) {
                        Expr::Call {
                            callee: Box::new(Expr::Ident(coerce_name.to_string())),
                            args: vec![Expr::Ident(field_name.clone())],
                            kwargs: vec![],
                        }
                    } else {
                        Expr::Ident(field_name.clone())
                    };
                    init_body.push(Stmt::SetAttr {
                        object: Expr::Ident("self".to_string()),
                        name: field_name.clone(),
                        value: coerce_expr,
                    });
                }
                let init_fn = Value::Function {
                    name: "__init__".to_string(),
                    params,
                    body: init_body,
                    closure: env.clone(),
                };
                let mut methods = HashMap::new();
                methods.insert("__init__".to_string(), init_fn);
                let cls = Rc::new(CoolClass {
                    name: name.clone(),
                    parent: None,
                    methods,
                });
                env.set_local(name.clone(), Value::Class(cls));
                Ok(Signal::None)
            }

            Stmt::Union { name, fields } => {
                // Lower union to a class with zero-defaulted fields.
                // Memory-sharing semantics are LLVM-only; in the interpreter each field is independent.
                let mut init_body = Vec::new();
                let mut params = vec![crate::ast::Param {
                    name: "self".to_string(),
                    default: None,
                    is_vararg: false,
                    is_kwarg: false,
                    type_name: None,
                }];
                for (field_name, type_name) in fields {
                    let zero_default = match type_name.as_str() {
                        "f32" | "f64" | "float" => Expr::Float(0.0),
                        "bool" => Expr::Bool(false),
                        _ => Expr::Int(0),
                    };
                    params.push(crate::ast::Param {
                        name: field_name.clone(),
                        default: Some(zero_default),
                        is_vararg: false,
                        is_kwarg: false,
                        type_name: None,
                    });
                    let coerce_name = match type_name.as_str() {
                        "f32" | "f64" => "float",
                        other => other,
                    };
                    let coerce_expr = if matches!(
                        type_name.as_str(),
                        "i8" | "u8"
                            | "i16"
                            | "u16"
                            | "i32"
                            | "u32"
                            | "i64"
                            | "u64"
                            | "isize"
                            | "usize"
                            | "f32"
                            | "f64"
                            | "float"
                            | "bool"
                    ) {
                        Expr::Call {
                            callee: Box::new(Expr::Ident(coerce_name.to_string())),
                            args: vec![Expr::Ident(field_name.clone())],
                            kwargs: vec![],
                        }
                    } else {
                        Expr::Ident(field_name.clone())
                    };
                    init_body.push(Stmt::SetAttr {
                        object: Expr::Ident("self".to_string()),
                        name: field_name.clone(),
                        value: coerce_expr,
                    });
                }
                let init_fn = Value::Function {
                    name: "__init__".to_string(),
                    params,
                    body: init_body,
                    closure: env.clone(),
                };
                let mut methods = HashMap::new();
                methods.insert("__init__".to_string(), init_fn);
                let cls = Rc::new(CoolClass {
                    name: name.clone(),
                    parent: None,
                    methods,
                });
                env.set_local(name.clone(), Value::Class(cls));
                Ok(Signal::None)
            }

            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                // exec_block may return Err("__raise__") when an exception crossed
                // a call boundary — recover the value from pending_raise.
                let block_result: Result<Signal, String> = match self.exec_block(body, env) {
                    Err(e) if e == "__raise__" => {
                        let v = self.pending_raise.take().unwrap_or(Value::Nil);
                        Ok(Signal::Raise(v))
                    }
                    // Also catch plain Err (e.g., "division by zero") from same scope
                    Err(msg) => Ok(Signal::Raise(Value::Str(msg))),
                    Ok(sig) => Ok(sig),
                };
                let result = block_result?;
                let sig = match result {
                    Signal::Raise(exc_val) => {
                        // Try to match an exception handler
                        let mut handled = false;
                        let mut handler_sig = Signal::None;
                        for handler in handlers {
                            let matches = match &handler.exc_type {
                                None => true, // bare except
                                Some(type_name) => {
                                    // Match by class name or string
                                    exc_matches(&exc_val, type_name)
                                }
                            };
                            if matches {
                                let handler_env = Env::new_child(env.clone());
                                if let Some(as_name) = &handler.as_name {
                                    handler_env.set_local(as_name.clone(), exc_val.clone());
                                }
                                handler_sig = self.exec_block(&handler.body, &handler_env)?;
                                handled = true;
                                break;
                            }
                        }
                        if !handled {
                            // Re-raise
                            if let Some(finally) = finally_body {
                                self.exec_block(finally, env)?;
                            }
                            return Ok(Signal::Raise(exc_val));
                        }
                        handler_sig
                    }
                    Signal::None => {
                        // No exception — run else body
                        if let Some(else_b) = else_body {
                            self.exec_block(else_b, env)?
                        } else {
                            Signal::None
                        }
                    }
                    other => other,
                };
                // Always run finally
                if let Some(finally) = finally_body {
                    let fsig = self.exec_block(finally, env)?;
                    // finally overrides other signals only if it produces one
                    if !matches!(fsig, Signal::None) {
                        return Ok(fsig);
                    }
                }
                Ok(sig)
            }

            // if/elif/else share the enclosing scope (Python semantics)
            Stmt::If {
                condition,
                then_body,
                elif_clauses,
                else_body,
            } => {
                if self.eval(condition, env)?.is_truthy() {
                    return self.exec_block(then_body, env);
                }
                for (cond, body) in elif_clauses {
                    if self.eval(cond, env)?.is_truthy() {
                        return self.exec_block(body, env);
                    }
                }
                if let Some(body) = else_body {
                    return self.exec_block(body, env);
                }
                Ok(Signal::None)
            }

            Stmt::While { condition, body } => {
                loop {
                    if !self.eval(condition, env)?.is_truthy() {
                        break;
                    }
                    match self.exec_block(body, env)? {
                        Signal::Break => break,
                        Signal::Continue => continue,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Raise(v) => return Ok(Signal::Raise(v)),
                        Signal::None => {}
                    }
                }
                Ok(Signal::None)
            }

            Stmt::For { var, iter, body } => {
                let iter_val = self.eval(iter, env)?;
                let items = self.to_iterable(iter_val)?;
                'outer: for item in items {
                    env.set_local(var.clone(), item);
                    match self.exec_block(body, env)? {
                        Signal::Break => break 'outer,
                        Signal::Continue => continue 'outer,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Raise(v) => return Ok(Signal::Raise(v)),
                        Signal::None => {}
                    }
                }
                Ok(Signal::None)
            }

            Stmt::Import(path) => {
                let full_path = if std::path::Path::new(path).is_absolute() {
                    std::path::PathBuf::from(path)
                } else {
                    self.source_dir.join(path)
                };
                let source =
                    std::fs::read_to_string(&full_path).map_err(|e| self.err(&format!("import error: {}", e)))?;

                let mut lexer = crate::lexer::Lexer::new(&source);
                let tokens = lexer
                    .tokenize()
                    .map_err(|e| self.err(&format!("import parse error: {}", e)))?;
                let mut parser = crate::parser::Parser::new(tokens);
                let program = parser
                    .parse_program()
                    .map_err(|e| self.err(&format!("import parse error: {}", e)))?;

                let child = Env::new_child(env.clone());
                self.exec_block(&program, &child)?;
                let child_vars = child.0.borrow();
                for (k, v) in &child_vars.vars {
                    env.set_local(k.clone(), v.clone());
                }
                Ok(Signal::None)
            }

            Stmt::ImportModule(name) => {
                self.import_module(name, env)?;
                Ok(Signal::None)
            }

            Stmt::Assert { condition, message } => {
                if !self.eval(condition, env)?.is_truthy() {
                    let msg = if let Some(m) = message {
                        format!("{}", self.eval(m, env)?)
                    } else {
                        "assertion failed".to_string()
                    };
                    return Ok(Signal::Raise(Value::Str(format!("AssertionError: {}", msg))));
                }
                Ok(Signal::None)
            }

            Stmt::With { expr, as_name, body } => {
                let manager = self.eval(expr, env)?;
                let entered = self.call_method(manager.clone(), "__enter__", vec![], vec![], env)?;
                let with_env = Env::new_child(env.clone());
                if let Some(name) = as_name {
                    with_env.set_local(name.clone(), entered);
                }
                let body_result = self.exec_block(body, &with_env);
                let exit_result = self.call_method(
                    manager,
                    "__exit__",
                    vec![Value::Nil, Value::Nil, Value::Nil],
                    vec![],
                    env,
                );
                match (body_result, exit_result) {
                    (Ok(sig), Ok(_)) => Ok(sig),
                    (Err(err), Ok(_)) => Err(err),
                    (_, Err(exit_err)) => Err(exit_err),
                }
            }

            Stmt::Global(names) => {
                // Mark these names as referencing the global scope
                for name in names {
                    env.declare_global(name.clone());
                }
                Ok(Signal::None)
            }

            Stmt::Nonlocal(names) => {
                // Mark these names as referencing the enclosing scope
                for name in names {
                    env.declare_nonlocal(name.clone());
                }
                Ok(Signal::None)
            }
        }
    }

    fn import_module(&mut self, name: &str, env: &Env) -> Result<(), String> {
        let binding_name = name.rsplit('.').next().unwrap_or(name);
        match name {
            "math" => {
                let mut map = IndexedMap::new();
                macro_rules! math_fn {
                    ($n:expr) => {
                        map.set(
                            Value::Str($n.to_string()),
                            Value::BuiltinFn(format!("math.{}", $n)),
                        );
                    };
                }
                math_fn!("sqrt");
                math_fn!("floor");
                math_fn!("ceil");
                math_fn!("round");
                math_fn!("abs");
                math_fn!("pow");
                math_fn!("log");
                math_fn!("log2");
                math_fn!("log10");
                math_fn!("sin");
                math_fn!("cos");
                math_fn!("tan");
                math_fn!("asin");
                math_fn!("acos");
                math_fn!("atan");
                math_fn!("atan2");
                math_fn!("exp");
                math_fn!("exp2");
                math_fn!("sinh");
                math_fn!("cosh");
                math_fn!("tanh");
                math_fn!("hypot");
                math_fn!("degrees");
                math_fn!("radians");
                math_fn!("trunc");
                math_fn!("gcd");
                math_fn!("lcm");
                math_fn!("factorial");
                math_fn!("isnan");
                math_fn!("isinf");
                math_fn!("isfinite");
                map.set(Value::Str("pi".to_string()), Value::Float(std::f64::consts::PI));
                map.set(Value::Str("tau".to_string()), Value::Float(std::f64::consts::TAU));
                map.set(Value::Str("e".to_string()), Value::Float(std::f64::consts::E));
                map.set(Value::Str("inf".to_string()), Value::Float(f64::INFINITY));
                map.set(Value::Str("nan".to_string()), Value::Float(f64::NAN));
                env.set_local("math".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
                // Also pull common names into scope directly
                env.set_local("sqrt".to_string(), Value::BuiltinFn("math.sqrt".to_string()));
                env.set_local("floor".to_string(), Value::BuiltinFn("math.floor".to_string()));
                env.set_local("ceil".to_string(), Value::BuiltinFn("math.ceil".to_string()));
                env.set_local("pi".to_string(), Value::Float(std::f64::consts::PI));
                env.set_local("tau".to_string(), Value::Float(std::f64::consts::TAU));
            }
            "os" => {
                let mut map = IndexedMap::new();
                macro_rules! os_fn {
                    ($n:expr) => {
                        map.set(
                            Value::Str($n.to_string()),
                            Value::BuiltinFn(format!("os.{}", $n)),
                        );
                    };
                }
                os_fn!("getcwd");
                os_fn!("listdir");
                os_fn!("exists");
                os_fn!("isdir");
                os_fn!("getenv");
                os_fn!("join");
                os_fn!("path");
                os_fn!("mkdir");
                os_fn!("remove");
                os_fn!("rename");
                os_fn!("popen");
                env.set_local("os".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
                env.set_local("getcwd".to_string(), Value::BuiltinFn("os.getcwd".to_string()));
                env.set_local("listdir".to_string(), Value::BuiltinFn("os.listdir".to_string()));
            }
            "sys" => {
                let mut argv: Vec<Value> = Vec::new();
                if let Ok(script_path) = std::env::var("COOL_SCRIPT_PATH") {
                    argv.push(Value::Str(script_path));
                } else {
                    argv.extend(std::env::args().map(Value::Str));
                }
                if let Ok(extra) = std::env::var("COOL_PROGRAM_ARGS") {
                    if !extra.is_empty() {
                        argv.extend(extra.split('\x1F').map(|arg| Value::Str(arg.to_string())));
                    }
                }
                let mut map = IndexedMap::new();
                map.set(Value::Str("argv".to_string()), Value::List(Rc::new(RefCell::new(argv))));
                map.set(Value::Str("exit".to_string()), Value::BuiltinFn("exit".to_string()));
                env.set_local("sys".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "path" => {
                let mut map = IndexedMap::new();
                for fn_name in &[
                    "join",
                    "basename",
                    "dirname",
                    "ext",
                    "stem",
                    "split",
                    "normalize",
                    "exists",
                    "isabs",
                ] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("path.{}", fn_name)),
                    );
                }
                env.set_local("path".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "platform" => {
                let mut map = IndexedMap::new();
                for fn_name in &[
                    "os",
                    "arch",
                    "family",
                    "runtime",
                    "exe_ext",
                    "shared_lib_ext",
                    "path_sep",
                    "newline",
                    "is_windows",
                    "is_unix",
                    "has_ffi",
                    "has_raw_memory",
                    "has_extern",
                    "has_inline_asm",
                ] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("platform.{}", fn_name)),
                    );
                }
                env.set_local("platform".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "core" => {
                let mut map = IndexedMap::new();
                for fn_name in &[
                    "word_bits",
                    "word_bytes",
                    "page_size",
                    "page_align_down",
                    "page_align_up",
                    "page_offset",
                    "page_index",
                    "page_count",
                    "pt_index",
                    "pd_index",
                    "pdpt_index",
                    "pml4_index",
                    "alloc",
                    "free",
                    "set_allocator",
                    "clear_allocator",
                ] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("core.{}", fn_name)),
                    );
                }
                env.set_local("core".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "string" => {
                let mut map = IndexedMap::new();
                for fn_name in &[
                    "split",
                    "join",
                    "strip",
                    "lstrip",
                    "rstrip",
                    "upper",
                    "lower",
                    "replace",
                    "startswith",
                    "endswith",
                    "find",
                    "count",
                    "format",
                    "title",
                    "capitalize",
                ] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("string.{}", fn_name)),
                    );
                }
                env.set_local("string".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "list" => {
                let mut map = IndexedMap::new();
                for fn_name in &["sort", "reverse", "filter", "map", "reduce", "flatten", "unique"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("list.{}", fn_name)),
                    );
                }
                env.set_local("list".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "json" => {
                let mut map = IndexedMap::new();
                map.set(
                    Value::Str("loads".to_string()),
                    Value::BuiltinFn("json.loads".to_string()),
                );
                map.set(
                    Value::Str("dumps".to_string()),
                    Value::BuiltinFn("json.dumps".to_string()),
                );
                env.set_local("json".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "toml" => {
                let mut map = IndexedMap::new();
                map.set(
                    Value::Str("loads".to_string()),
                    Value::BuiltinFn("toml.loads".to_string()),
                );
                map.set(
                    Value::Str("dumps".to_string()),
                    Value::BuiltinFn("toml.dumps".to_string()),
                );
                env.set_local("toml".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "yaml" => {
                let mut map = IndexedMap::new();
                map.set(
                    Value::Str("loads".to_string()),
                    Value::BuiltinFn("yaml.loads".to_string()),
                );
                map.set(
                    Value::Str("dumps".to_string()),
                    Value::BuiltinFn("yaml.dumps".to_string()),
                );
                env.set_local("yaml".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "sqlite" => {
                let mut map = IndexedMap::new();
                for fn_name in &["execute", "query", "scalar"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("sqlite.{}", fn_name)),
                    );
                }
                env.set_local("sqlite".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "http" => {
                let mut map = IndexedMap::new();
                for fn_name in &["get", "post", "head", "getjson"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("http.{}", fn_name)),
                    );
                }
                env.set_local("http".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "re" => {
                let mut map = IndexedMap::new();
                for fn_name in &["match", "search", "fullmatch", "findall", "sub", "split"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("re.{}", fn_name)),
                    );
                }
                env.set_local("re".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "time" => {
                let mut map = IndexedMap::new();
                for fn_name in &["time", "sleep", "monotonic"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("time.{}", fn_name)),
                    );
                }
                env.set_local("time".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "random" => {
                let mut map = IndexedMap::new();
                for fn_name in &["random", "randint", "choice", "shuffle", "uniform", "seed"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("random.{}", fn_name)),
                    );
                }
                env.set_local("random".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "subprocess" => {
                let mut map = IndexedMap::new();
                for fn_name in &["run", "call", "check_output"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("subprocess.{}", fn_name)),
                    );
                }
                env.set_local("subprocess".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "argparse" => {
                let mut map = IndexedMap::new();
                for fn_name in &["parse", "help"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("argparse.{}", fn_name)),
                    );
                }
                env.set_local("argparse".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "csv" => {
                let mut map = IndexedMap::new();
                for fn_name in &["rows", "dicts", "write"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("csv.{}", fn_name)),
                    );
                }
                env.set_local("csv".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "datetime" => {
                let mut map = IndexedMap::new();
                for fn_name in &["now", "format", "parse", "parts", "add_seconds", "diff_seconds"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("datetime.{}", fn_name)),
                    );
                }
                env.set_local("datetime".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "hashlib" => {
                let mut map = IndexedMap::new();
                for fn_name in &["md5", "sha1", "sha256", "digest"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("hashlib.{}", fn_name)),
                    );
                }
                env.set_local("hashlib".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "test" => {
                let mut map = IndexedMap::new();
                for fn_name in &[
                    "equal",
                    "not_equal",
                    "truthy",
                    "falsey",
                    "is_nil",
                    "not_nil",
                    "fail",
                    "raises",
                ] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("test.{}", fn_name)),
                    );
                }
                env.set_local("test".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "logging" => {
                let mut map = IndexedMap::new();
                for fn_name in &["basic_config", "log", "debug", "info", "warning", "warn", "error"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("logging.{}", fn_name)),
                    );
                }
                env.set_local("logging".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "socket" => {
                let mut map = IndexedMap::new();
                for fn_name in &["connect", "listen"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("socket.{}", fn_name)),
                    );
                }
                env.set_local("socket".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "term" => {
                let mut map = IndexedMap::new();
                for fn_name in &[
                    "raw",
                    "normal",
                    "clear",
                    "clear_line",
                    "move_cursor",
                    "hide_cursor",
                    "show_cursor",
                    "write",
                    "flush",
                    "poll_char",
                    "get_char",
                    "size",
                ] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("term.{}", fn_name)),
                    );
                }
                env.set_local("term".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "collections" => {
                // Implemented as Cool code
                let src = r#"
class Queue:
    def __init__(self):
        self.items = []

    def push(self, item):
        self.items.append(item)

    def enqueue(self, item):
        self.push(item)

    def pop(self):
        if len(self.items) == 0:
            raise "Queue is empty"
        item = self.items[0]
        self.items = self.items[1:]
        return item

    def dequeue(self):
        return self.pop()

    def peek(self):
        if len(self.items) == 0:
            raise "Queue is empty"
        return self.items[0]

    def is_empty(self):
        return len(self.items) == 0

    def size(self):
        return len(self.items)

class Stack:
    def __init__(self):
        self.items = []

    def push(self, item):
        self.items.append(item)

    def pop(self):
        if len(self.items) == 0:
            raise "Stack is empty"
        return self.items.pop()

    def peek(self):
        if len(self.items) == 0:
            raise "Stack is empty"
        return self.items[len(self.items) - 1]

    def is_empty(self):
        return len(self.items) == 0

    def size(self):
        return len(self.items)
"#;
                let mut lexer = crate::lexer::Lexer::new(src);
                let tokens = lexer.tokenize().map_err(|e| self.err(&e))?;
                let mut parser = crate::parser::Parser::new(tokens);
                let program = parser.parse_program().map_err(|e| self.err(&e))?;
                self.exec_block(&program, env)?;
                let mut map = IndexedMap::new();
                if let Some(queue) = env.get("Queue") {
                    map.set(Value::Str("Queue".to_string()), queue);
                }
                if let Some(stack) = env.get("Stack") {
                    map.set(Value::Str("Stack".to_string()), stack);
                }
                env.set_local("collections".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            "ffi" => {
                let mut map = IndexedMap::new();
                for fn_name in &["open", "func"] {
                    map.set(
                        Value::Str(fn_name.to_string()),
                        Value::BuiltinFn(format!("ffi.{}", fn_name)),
                    );
                }
                env.set_local("ffi".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            _ => {
                if let Some(path) = self.module_resolver.resolve_module(&self.source_dir, name) {
                    let source =
                        std::fs::read_to_string(&path).map_err(|e| self.err(&format!("import error: {}", e)))?;
                    let mut lexer = crate::lexer::Lexer::new(&source);
                    let tokens = lexer.tokenize().map_err(|e| self.err(&e))?;
                    let mut parser = crate::parser::Parser::new(tokens);
                    let program = parser.parse_program().map_err(|e| self.err(&e))?;
                    let child = Env::new_child(env.clone());
                    let old_dir = self.source_dir.clone();
                    self.source_dir = path.parent().unwrap_or(&old_dir).to_path_buf();
                    let result = self.exec_block(&program, &child);
                    self.source_dir = old_dir;
                    result?;
                    let child_vars = child.0.borrow();
                    let mut exports = IndexedMap::new();
                    for (k, v) in &child_vars.vars {
                        exports.set(Value::Str(k.clone()), v.clone());
                    }
                    env.set_local(binding_name.to_string(), Value::Dict(Rc::new(RefCell::new(exports))));
                } else {
                    return Err(self.err(&format!("unknown module '{}'", name)));
                }
            }
        }
        Ok(())
    }

    fn to_iterable(&self, v: Value) -> Result<Vec<Value>, String> {
        match v {
            Value::List(lst) => Ok(lst.borrow().clone()),
            Value::Tuple(t) => Ok((*t).clone()),
            Value::Str(s) => Ok(s.chars().map(|c| Value::Str(c.to_string())).collect()),
            Value::Dict(map) => Ok(map.borrow().keys.clone()),
            Value::Int(n) => Err(self.err(&format!("cannot iterate over int (did you mean range({}))?", n))),
            other => Err(self.err(&format!("cannot iterate over {}", other.type_name()))),
        }
    }

    fn eval(&mut self, expr: &Expr, env: &Env) -> Result<Value, String> {
        match expr {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nil => Ok(Value::Nil),

            Expr::List(items) => {
                let vs: Result<Vec<_>, _> = items.iter().map(|e| self.eval(e, env)).collect();
                Ok(Value::List(Rc::new(RefCell::new(vs?))))
            }

            Expr::Tuple(items) => {
                let vs: Result<Vec<_>, _> = items.iter().map(|e| self.eval(e, env)).collect();
                Ok(Value::Tuple(Rc::new(vs?)))
            }

            Expr::Dict(pairs) => {
                let mut map = IndexedMap::new();
                for (k_expr, v_expr) in pairs {
                    let k = self.eval(k_expr, env)?;
                    let v = self.eval(v_expr, env)?;
                    map.set(k, v);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(map))))
            }

            Expr::FString(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        FStringPart::Literal(s) => result.push_str(s),
                        FStringPart::Expr(e) => {
                            let v = self.eval(e, env)?;
                            result.push_str(&format!("{}", v));
                        }
                    }
                }
                Ok(Value::Str(result))
            }

            Expr::Lambda { params, body } => Ok(Value::Function {
                name: "<lambda>".to_string(),
                params: params.clone(),
                body: vec![Stmt::Return(Some(*body.clone()))],
                closure: env.clone(),
            }),

            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                if self.eval(condition, env)?.is_truthy() {
                    self.eval(then_expr, env)
                } else {
                    self.eval(else_expr, env)
                }
            }

            Expr::ListComp {
                expr,
                var,
                iter,
                condition,
            } => {
                let iter_val = self.eval(iter, env)?;
                let items = self.to_iterable(iter_val)?;
                let mut result = Vec::new();
                for item in items {
                    let comp_env = Env::new_child(env.clone());
                    comp_env.set_local(var.clone(), item);
                    if let Some(cond) = condition {
                        if !self.eval(cond, &comp_env)?.is_truthy() {
                            continue;
                        }
                    }
                    result.push(self.eval(expr, &comp_env)?);
                }
                Ok(Value::List(Rc::new(RefCell::new(result))))
            }

            Expr::Ident(name) => env
                .get(name)
                .ok_or_else(|| self.err(&format!("undefined variable '{}'", name))),

            Expr::UnaryOp { op, expr } => {
                let v = self.eval(expr, env)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        other => Err(self.err(&format!("cannot negate {}", other.type_name()))),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!v.is_truthy())),
                    UnaryOp::BitNot => match v {
                        Value::Int(n) => Ok(Value::Int(!n)),
                        other => Err(self.err(&format!("bitwise ~ requires int, got {}", other.type_name()))),
                    },
                }
            }

            Expr::BinOp { op, left, right } => {
                match op {
                    BinOp::And => {
                        let lv = self.eval(left, env)?;
                        return if !lv.is_truthy() { Ok(lv) } else { self.eval(right, env) };
                    }
                    BinOp::Or => {
                        let lv = self.eval(left, env)?;
                        return if lv.is_truthy() { Ok(lv) } else { self.eval(right, env) };
                    }
                    BinOp::In => {
                        let item = self.eval(left, env)?;
                        let container = self.eval(right, env)?;
                        return Ok(Value::Bool(self.contains_value(&container, &item)?));
                    }
                    BinOp::NotIn => {
                        let item = self.eval(left, env)?;
                        let container = self.eval(right, env)?;
                        return Ok(Value::Bool(!self.contains_value(&container, &item)?));
                    }
                    _ => {}
                }
                let l = self.eval(left, env)?;
                let r = self.eval(right, env)?;
                // Operator overloading for instances
                let dunder = match op {
                    BinOp::Add => Some("__add__"),
                    BinOp::Sub => Some("__sub__"),
                    BinOp::Mul => Some("__mul__"),
                    BinOp::Div => Some("__div__"),
                    BinOp::Mod => Some("__mod__"),
                    BinOp::Pow => Some("__pow__"),
                    BinOp::Eq => Some("__eq__"),
                    BinOp::Lt => Some("__lt__"),
                    BinOp::GtEq => Some("__ge__"),
                    BinOp::LtEq => Some("__le__"),
                    BinOp::Gt => Some("__gt__"),
                    _ => None,
                };
                if let (Some(dname), Value::Instance(inst)) = (dunder, &l) {
                    if let Some(func) = lookup_method(&inst.class, dname) {
                        return self.call_value(func, vec![l, r], vec![], env);
                    }
                }
                self.apply_binop(op, l, r)
            }

            // Method call: obj.method(args) — dispatched before generic Call
            Expr::Call { callee, args, kwargs } if matches!(**callee, Expr::Attr { .. }) => {
                if let Expr::Attr { object, name } = callee.as_ref() {
                    let obj = self.eval(object, env)?;
                    let arg_vals: Result<Vec<_>, _> = args.iter().map(|a| self.eval(a, env)).collect();
                    let arg_vals = arg_vals?;
                    let kwarg_vals = self.eval_kwargs(kwargs, env)?;
                    if let Value::Dict(map) = &obj {
                        if let Some(func) = map.borrow().get(&Value::Str(name.clone())) {
                            return self.call_value(func, arg_vals, kwarg_vals, env);
                        }
                    }
                    return self.call_method(obj, name, arg_vals, kwarg_vals, env);
                }
                unreachable!()
            }

            Expr::Call { callee, args, kwargs } => {
                let func = self.eval(callee, env)?;
                let arg_vals: Result<Vec<_>, _> = args.iter().map(|a| self.eval(a, env)).collect();
                let kwarg_vals = self.eval_kwargs(kwargs, env)?;
                self.call_value(func, arg_vals?, kwarg_vals, env)
            }

            Expr::Index { object, index } => {
                let obj = self.eval(object, env)?;
                let idx = self.eval(index, env)?;
                self.eval_index(obj, idx)
            }

            Expr::Slice { object, start, stop } => {
                let obj = self.eval(object, env)?;
                let len = match &obj {
                    Value::List(lst) => lst.borrow().len() as i64,
                    Value::Str(s) => s.chars().count() as i64,
                    Value::Tuple(t) => t.len() as i64,
                    other => {
                        return Err(self.err(&format!("'{}' is not sliceable", other.type_name())));
                    }
                };
                let start_i: usize = if let Some(expr) = start {
                    match self.eval(expr, env)? {
                        Value::Int(n) => {
                            let n = if n < 0 { (len + n).max(0) } else { n.min(len) };
                            n as usize
                        }
                        other => {
                            return Err(self.err(&format!("slice index must be int, got {}", other.type_name())));
                        }
                    }
                } else {
                    0
                };
                let stop_i: usize = if let Some(expr) = stop {
                    match self.eval(expr, env)? {
                        Value::Int(n) => {
                            let n = if n < 0 { (len + n).max(0) } else { n.min(len) };
                            n as usize
                        }
                        other => {
                            return Err(self.err(&format!("slice index must be int, got {}", other.type_name())));
                        }
                    }
                } else {
                    len as usize
                };
                let stop_i = stop_i.min(len as usize);
                let start_i = start_i.min(stop_i);
                match obj {
                    Value::List(lst) => {
                        let slice: Vec<Value> = lst.borrow()[start_i..stop_i].to_vec();
                        Ok(Value::List(Rc::new(RefCell::new(slice))))
                    }
                    Value::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let slice: String = chars[start_i..stop_i].iter().collect();
                        Ok(Value::Str(slice))
                    }
                    Value::Tuple(t) => {
                        let slice: Vec<Value> = t[start_i..stop_i].to_vec();
                        Ok(Value::Tuple(Rc::new(slice)))
                    }
                    _ => unreachable!(),
                }
            }

            Expr::Attr { object, name } => {
                let obj = self.eval(object, env)?;
                match &obj {
                    Value::Instance(inst) => {
                        // Check instance fields first, then class methods
                        if let Some(v) = inst.fields.borrow().get(name) {
                            return Ok(v.clone());
                        }
                        // Look up method in class chain
                        if let Some(method) = lookup_method(&inst.class, name) {
                            // Return a bound method (closure wrapping self)
                            return Ok(method);
                        }
                        Err(self.err(&format!("'{}' has no attribute '{}'", inst.class.name, name)))
                    }
                    Value::Dict(map) => {
                        // First look up the attribute as a dict key (e.g. module.member)
                        if let Some(v) = map.borrow().get(&Value::Str(name.clone())) {
                            return Ok(v);
                        }
                        // Fall back to a method stub (keys, values, items, etc.)
                        Ok(Value::BuiltinFn(format!("<method {} on dict>", name)))
                    }
                    Value::Str(_) | Value::List(_) | Value::File(_) | Value::Socket(_) => {
                        Ok(Value::BuiltinFn(format!("<method {} on {}>", name, obj.type_name())))
                    }
                    Value::Class(cls) => {
                        if let Some(v) = cls.methods.get(name) {
                            return Ok(v.clone());
                        }
                        Err(self.err(&format!("class '{}' has no attribute '{}'", cls.name, name)))
                    }
                    other => Err(self.err(&format!("'{}' has no attribute '{}'", other.type_name(), name))),
                }
            }
        }
    }

    fn contains_value(&self, container: &Value, item: &Value) -> Result<bool, String> {
        match container {
            Value::List(lst) => Ok(lst.borrow().iter().any(|v| values_equal(v, item))),
            Value::Tuple(t) => Ok(t.iter().any(|v| values_equal(v, item))),
            Value::Str(s) => {
                if let Value::Str(sub) = item {
                    Ok(s.contains(sub.as_str()))
                } else {
                    Err(self.err("'in' requires string for string containment check"))
                }
            }
            Value::Dict(map) => Ok(map.borrow().contains(item)),
            other => Err(self.err(&format!("'in' not supported for {}", other.type_name()))),
        }
    }

    fn eval_index(&self, obj: Value, idx: Value) -> Result<Value, String> {
        match obj {
            Value::List(lst) => {
                let i = to_list_index(&lst.borrow(), idx, self.current_line)?;
                Ok(lst.borrow()[i].clone())
            }
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                let i = index_into(chars.len(), &idx, self.current_line)?;
                Ok(Value::Str(chars[i].to_string()))
            }
            Value::Dict(map) => map
                .borrow()
                .get(&idx)
                .ok_or_else(|| self.err(&format!("key {} not found in dict", repr(&idx)))),
            Value::Tuple(t) => {
                let i = index_into(t.len(), &idx, self.current_line)?;
                Ok(t[i].clone())
            }
            other => Err(self.err(&format!("cannot index into {}", other.type_name()))),
        }
    }

    // ── Method dispatch ───────────────────────────────────────────────────

    fn call_method(
        &mut self,
        obj: Value,
        method: &str,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
        env: &Env,
    ) -> Result<Value, String> {
        // For dicts, first check if the dict stores a callable under that key
        // (e.g. module dicts like `ffi`, `math`, `os`).
        if let Value::Dict(ref map) = obj {
            let callable = map.borrow().get(&Value::Str(method.to_string()));
            if let Some(func @ (Value::BuiltinFn(_) | Value::Function { .. } | Value::FfiFunc { .. })) = callable {
                return self.call_value(func, args, kwargs, env);
            }
        }
        match &obj {
            Value::Str(s) => self.str_method(s.clone(), method, args),
            Value::List(_) => self.list_method(obj, method, args),
            Value::Dict(_) => self.dict_method(obj, method, args),
            Value::File(_) => self.file_method(obj, method, args),
            Value::Socket(_) => self.socket_method(obj, method, args),
            Value::Instance(_) => self.instance_method(obj, method, args, kwargs, env),
            Value::Super { .. } => self.super_method(obj, method, args, kwargs, env),
            other => Err(self.err(&format!("'{}' has no method '{}'", other.type_name(), method))),
        }
    }

    fn super_method(
        &mut self,
        obj: Value,
        method: &str,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
        env: &Env,
    ) -> Result<Value, String> {
        let (inst, parent) = match obj {
            Value::Super { instance, parent } => (instance, parent),
            _ => unreachable!(),
        };
        if let Some(func) = lookup_method(&parent, method) {
            let mut full_args = vec![Value::Instance(inst)];
            full_args.extend(args);
            return self.call_value(func, full_args, kwargs, env);
        }
        Err(self.err(&format!("super(): parent has no method '{}'", method)))
    }

    fn instance_method(
        &mut self,
        obj: Value,
        method: &str,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
        env: &Env,
    ) -> Result<Value, String> {
        let inst = match &obj {
            Value::Instance(i) => i.clone(),
            _ => unreachable!(),
        };
        // Look up method in class chain
        if let Some(func) = lookup_method(&inst.class, method) {
            // Call with self as first argument
            let mut full_args = vec![obj.clone()];
            full_args.extend(args);
            return self.call_value(func, full_args, kwargs, env);
        }
        Err(self.err(&format!("'{}' has no method '{}'", inst.class.name, method)))
    }

    fn str_method(&self, s: String, method: &str, args: Vec<Value>) -> Result<Value, String> {
        match method {
            "upper" => Ok(Value::Str(s.to_uppercase())),
            "lower" => Ok(Value::Str(s.to_lowercase())),
            "strip" => Ok(Value::Str(s.trim().to_string())),
            "lstrip" => Ok(Value::Str(s.trim_start().to_string())),
            "rstrip" => Ok(Value::Str(s.trim_end().to_string())),
            "len" => Ok(Value::Int(s.chars().count() as i64)),
            "startswith" => {
                let pat = req_str_arg(&args, 0, "startswith")?;
                Ok(Value::Bool(s.starts_with(pat.as_str())))
            }
            "endswith" => {
                let pat = req_str_arg(&args, 0, "endswith")?;
                Ok(Value::Bool(s.ends_with(pat.as_str())))
            }
            "contains" => {
                let pat = req_str_arg(&args, 0, "contains")?;
                Ok(Value::Bool(s.contains(pat.as_str())))
            }
            "find" => {
                let pat = req_str_arg(&args, 0, "find")?;
                Ok(match s.find(pat.as_str()) {
                    Some(i) => Value::Int(s[..i].chars().count() as i64),
                    None => Value::Int(-1),
                })
            }
            "replace" => {
                let from = req_str_arg(&args, 0, "replace")?;
                let to = req_str_arg(&args, 1, "replace")?;
                Ok(Value::Str(s.replace(from.as_str(), to.as_str())))
            }
            "split" => {
                let parts: Vec<Value> = if args.is_empty() {
                    s.split_whitespace().map(|p| Value::Str(p.to_string())).collect()
                } else {
                    let sep = req_str_arg(&args, 0, "split")?;
                    s.split(sep.as_str()).map(|p| Value::Str(p.to_string())).collect()
                };
                Ok(Value::List(Rc::new(RefCell::new(parts))))
            }
            "join" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l,
                    _ => return Err(self.err("join() requires a list argument")),
                };
                let parts: Vec<String> = lst.borrow().iter().map(|v| v.to_string()).collect();
                Ok(Value::Str(parts.join(&s)))
            }
            "chars" => {
                let cs: Vec<Value> = s.chars().map(|c| Value::Str(c.to_string())).collect();
                Ok(Value::List(Rc::new(RefCell::new(cs))))
            }
            "repeat" => {
                let n = req_int_arg(&args, 0, "repeat")?;
                Ok(Value::Str(s.repeat(n.max(0) as usize)))
            }
            "to_int" => s
                .trim()
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| self.err(&format!("cannot convert \"{}\" to int", s))),
            "to_float" => s
                .trim()
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| self.err(&format!("cannot convert \"{}\" to float", s))),
            "format" => {
                // Simple format: replace {} placeholders with arguments
                let mut result = String::new();
                let mut arg_iter = args.iter();
                let mut chars = s.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '{' {
                        if chars.peek() == Some(&'}') {
                            chars.next();
                            match arg_iter.next() {
                                Some(v) => result.push_str(&v.to_string()),
                                None => result.push_str("{}"),
                            }
                        } else {
                            result.push(c);
                        }
                    } else {
                        result.push(c);
                    }
                }
                Ok(Value::Str(result))
            }
            "count" => {
                let pat = req_str_arg(&args, 0, "count")?;
                let cnt = s.matches(pat.as_str()).count();
                Ok(Value::Int(cnt as i64))
            }
            "isdigit" => Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))),
            "isalpha" => Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_alphabetic()))),
            "isalnum" => Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_alphanumeric()))),
            "isupper" => Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_uppercase()))),
            "islower" => Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_lowercase()))),
            _ => Err(self.err(&format!("str has no method '{}'", method))),
        }
    }

    fn list_method(&self, obj: Value, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let lst = match obj {
            Value::List(l) => l,
            _ => unreachable!(),
        };
        match method {
            "append" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("append() requires 1 argument"))?;
                lst.borrow_mut().push(v);
                Ok(Value::Nil)
            }
            "pop" => {
                let mut v = lst.borrow_mut();
                if let Some(idx) = args.first() {
                    let i = index_into(v.len(), idx, self.current_line)?;
                    Ok(v.remove(i))
                } else {
                    v.pop().ok_or_else(|| self.err("pop() on empty list"))
                }
            }
            "insert" => {
                let i = req_int_arg(&args, 0, "insert")? as usize;
                let val = args
                    .into_iter()
                    .nth(1)
                    .ok_or_else(|| self.err("insert() requires 2 arguments"))?;
                let mut v = lst.borrow_mut();
                let i = i.min(v.len());
                v.insert(i, val);
                Ok(Value::Nil)
            }
            "remove" => {
                let target = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("remove() requires 1 argument"))?;
                let mut v = lst.borrow_mut();
                if let Some(i) = v.iter().position(|x| values_equal(x, &target)) {
                    v.remove(i);
                    Ok(Value::Nil)
                } else {
                    Err(self.err(&format!("value {} not in list", repr(&target))))
                }
            }
            "contains" => {
                let target = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("contains() requires 1 argument"))?;
                Ok(Value::Bool(lst.borrow().iter().any(|x| values_equal(x, &target))))
            }
            "len" => Ok(Value::Int(lst.borrow().len() as i64)),
            "reverse" => {
                lst.borrow_mut().reverse();
                Ok(Value::Nil)
            }
            "sort" => {
                let mut v = lst.borrow_mut();
                let mut err: Option<String> = None;
                v.sort_by(|a, b| match compare_values(a, b) {
                    Ok(std::cmp::Ordering::Less) => std::cmp::Ordering::Less,
                    Ok(std::cmp::Ordering::Equal) => std::cmp::Ordering::Equal,
                    Ok(std::cmp::Ordering::Greater) => std::cmp::Ordering::Greater,
                    Err(e) => {
                        err = Some(e);
                        std::cmp::Ordering::Equal
                    }
                });
                if let Some(e) = err {
                    return Err(e);
                }
                Ok(Value::Nil)
            }
            "join" => {
                let sep = match args.into_iter().next() {
                    Some(Value::Str(s)) => s,
                    None => String::new(),
                    _ => return Err(self.err("join() separator must be a string")),
                };
                let parts: Vec<String> = lst.borrow().iter().map(|v| v.to_string()).collect();
                Ok(Value::Str(parts.join(&sep)))
            }
            "copy" => Ok(Value::List(Rc::new(RefCell::new(lst.borrow().clone())))),
            "clear" => {
                lst.borrow_mut().clear();
                Ok(Value::Nil)
            }
            "index" => {
                let target = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("index() requires 1 argument"))?;
                let v = lst.borrow();
                match v.iter().position(|x| values_equal(x, &target)) {
                    Some(i) => Ok(Value::Int(i as i64)),
                    None => Err(self.err(&format!("value {} not in list", repr(&target)))),
                }
            }
            "extend" => {
                let other = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("extend() requires 1 argument"))?;
                match other {
                    Value::List(other_lst) => {
                        let items = other_lst.borrow().clone();
                        lst.borrow_mut().extend(items);
                        Ok(Value::Nil)
                    }
                    _ => Err(self.err("extend() requires a list")),
                }
            }
            _ => Err(self.err(&format!("list has no method '{}'", method))),
        }
    }

    fn dict_method(&self, obj: Value, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let map = match obj {
            Value::Dict(m) => m,
            _ => unreachable!(),
        };
        match method {
            "keys" => {
                let keys = map.borrow().keys.clone();
                Ok(Value::List(Rc::new(RefCell::new(keys))))
            }
            "values" => {
                let vals = map.borrow().vals.clone();
                Ok(Value::List(Rc::new(RefCell::new(vals))))
            }
            "items" => {
                let m = map.borrow();
                let pairs: Vec<Value> = m
                    .keys
                    .iter()
                    .zip(m.vals.iter())
                    .map(|(k, v)| Value::List(Rc::new(RefCell::new(vec![k.clone(), v.clone()]))))
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(pairs))))
            }
            "get" => {
                let key = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("get() requires 1 argument"))?;
                Ok(map.borrow().get(&key).unwrap_or(Value::Nil))
            }
            "contains" | "has_key" => {
                let key = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("contains() requires 1 argument"))?;
                Ok(Value::Bool(map.borrow().contains(&key)))
            }
            "remove" | "del" => {
                let key = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("remove() requires 1 argument"))?;
                map.borrow_mut().remove(&key);
                Ok(Value::Nil)
            }
            "len" => Ok(Value::Int(map.borrow().keys.len() as i64)),
            "clear" => {
                let mut m = map.borrow_mut();
                m.keys.clear();
                m.vals.clear();
                Ok(Value::Nil)
            }
            "copy" => Ok(Value::Dict(Rc::new(RefCell::new(map.borrow().clone())))),
            "update" => {
                let other = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("update() requires 1 argument"))?;
                match other {
                    Value::Dict(other_map) => {
                        let other_b = other_map.borrow();
                        let pairs: Vec<_> = other_b
                            .keys
                            .iter()
                            .zip(other_b.vals.iter())
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        drop(other_b);
                        for (k, v) in pairs {
                            map.borrow_mut().set(k, v);
                        }
                        Ok(Value::Nil)
                    }
                    _ => Err(self.err("update() requires a dict")),
                }
            }
            _ => Err(self.err(&format!("dict has no method '{}'", method))),
        }
    }

    fn file_method(&self, obj: Value, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let fh_rc = match obj {
            Value::File(f) => f,
            _ => unreachable!(),
        };
        match method {
            "__enter__" => Ok(Value::File(fh_rc)),
            "__exit__" => self.file_method(Value::File(fh_rc), "close", vec![]),
            "read" => {
                let fh = fh_rc.borrow();
                if fh.closed {
                    return Err(self.err("read() on closed file"));
                }
                Ok(Value::Str(fh.content.join("\n")))
            }
            "readline" => {
                let mut fh = fh_rc.borrow_mut();
                if fh.closed {
                    return Err(self.err("readline() on closed file"));
                }
                if fh.line_pos >= fh.content.len() {
                    return Ok(Value::Str(String::new()));
                }
                let line = fh.content[fh.line_pos].clone() + "\n";
                fh.line_pos += 1;
                Ok(Value::Str(line))
            }
            "readlines" => {
                let fh = fh_rc.borrow();
                if fh.closed {
                    return Err(self.err("readlines() on closed file"));
                }
                let lines: Vec<Value> = fh.content.iter().map(|l| Value::Str(l.clone() + "\n")).collect();
                Ok(Value::List(Rc::new(RefCell::new(lines))))
            }
            "write" => {
                let text = match args.into_iter().next() {
                    Some(Value::Str(s)) => s,
                    Some(v) => v.to_string(),
                    None => return Err(self.err("write() requires 1 argument")),
                };
                let fh = fh_rc.borrow();
                if fh.closed {
                    return Err(self.err("write() on closed file"));
                }
                if fh.mode == "r" {
                    return Err(self.err("write() on read-only file"));
                }
                fh.write_buf.borrow_mut().push_str(&text);
                Ok(Value::Int(text.len() as i64))
            }
            "writelines" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => return Err(self.err("writelines() requires a list")),
                };
                let fh = fh_rc.borrow();
                if fh.closed {
                    return Err(self.err("writelines() on closed file"));
                }
                for item in &lst {
                    fh.write_buf.borrow_mut().push_str(&item.to_string());
                }
                Ok(Value::Nil)
            }
            "close" => {
                let mut fh = fh_rc.borrow_mut();
                if !fh.closed {
                    // Flush write buffer
                    if fh.mode != "r" && !fh.write_buf.borrow().is_empty() {
                        let content = fh.write_buf.borrow().clone();
                        std::fs::write(&fh.path, &content)
                            .map_err(|e| self.err(&format!("file write error: {}", e)))?;
                    }
                    fh.closed = true;
                }
                Ok(Value::Nil)
            }
            "flush" => {
                let fh = fh_rc.borrow();
                if !fh.closed && fh.mode != "r" {
                    let content = fh.write_buf.borrow().clone();
                    std::fs::write(&fh.path, &content).map_err(|e| self.err(&format!("file write error: {}", e)))?;
                }
                Ok(Value::Nil)
            }
            _ => Err(self.err(&format!("file has no method '{}'", method))),
        }
    }

    // ── Function call ─────────────────────────────────────────────────────

    fn call_value(
        &mut self,
        func: Value,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
        env: &Env,
    ) -> Result<Value, String> {
        match func {
            Value::BuiltinFn(name) => self.call_builtin(&name, args, env),
            Value::Function {
                params, body, closure, ..
            } => {
                let fn_env = Env::new_child(closure.clone());
                self.bind_args(&params, args, kwargs, &fn_env, &closure)?;
                match self.exec_block(&body, &fn_env)? {
                    Signal::Return(v) => Ok(v),
                    Signal::Break => Err(self.err("'break' outside loop")),
                    Signal::Continue => Err(self.err("'continue' outside loop")),
                    Signal::Raise(v) => {
                        self.pending_raise = Some(v);
                        Err("__raise__".to_string())
                    }
                    Signal::None => Ok(Value::Nil),
                }
            }
            Value::Class(cls) => {
                // Instantiate: create instance, call __init__ if defined
                let inst = Rc::new(CoolInstance {
                    class: cls.clone(),
                    fields: RefCell::new(HashMap::new()),
                });
                let inst_val = Value::Instance(inst);
                // Call __init__ if it exists
                if let Some(init_fn) = lookup_method(&cls, "__init__") {
                    let mut init_args = vec![inst_val.clone()];
                    init_args.extend(args);
                    self.call_value(init_fn, init_args, kwargs, env)?;
                }
                Ok(inst_val)
            }
            Value::FfiFunc {
                sym,
                ret_type,
                arg_types,
                ..
            } => {
                if !kwargs.is_empty() {
                    return Err(self.err("FFI functions do not support keyword arguments"));
                }
                self.invoke_ffi(sym, &ret_type, &arg_types, args)
            }
            other => Err(self.err(&format!("{} is not callable", other.type_name()))),
        }
    }

    fn bind_args(
        &mut self,
        params: &[Param],
        mut args: Vec<Value>,
        mut kwargs: Vec<(String, Value)>,
        env: &Env,
        closure: &Env,
    ) -> Result<(), String> {
        let mut positional_done = false;
        let mut i = 0;

        for param in params {
            if param.is_vararg {
                let rest: Vec<Value> = args.drain(i..).collect();
                env.set_local(param.name.clone(), Value::List(Rc::new(RefCell::new(rest))));
                positional_done = true;
                continue;
            }
            if param.is_kwarg {
                let mut map = IndexedMap::new();
                for (k, v) in kwargs.drain(..) {
                    map.set(Value::Str(k), v);
                }
                env.set_local(param.name.clone(), Value::Dict(Rc::new(RefCell::new(map))));
                continue;
            }

            // Check kwargs first — do NOT advance positional index i
            if let Some(pos) = kwargs.iter().position(|(k, _)| k == &param.name) {
                let (_, v) = kwargs.remove(pos);
                env.set_local(param.name.clone(), v);
                continue;
            }

            if i < args.len() {
                env.set_local(param.name.clone(), args[i].clone());
                i += 1;
            } else if let Some(default_expr) = &param.default {
                // Evaluate the default in the closure scope
                let default_expr = default_expr.clone();
                let val = self.eval(&default_expr, closure)?;
                env.set_local(param.name.clone(), val);
            } else {
                return Err(self.err(&format!("missing argument '{}'", param.name)));
            }
        }

        if i < args.len() && !positional_done {
            let expected = params.iter().filter(|p| !p.is_vararg && !p.is_kwarg).count();
            return Err(self.err(&format!("expected {} arg(s), got {}", expected, args.len())));
        }
        if !kwargs.is_empty() {
            return Err(self.err(&format!("unexpected keyword argument '{}'", kwargs[0].0)));
        }
        Ok(())
    }

    // ── Built-in functions ────────────────────────────────────────────────

    /// Evaluate kwargs, expanding `**dict` spreads into individual key-value pairs.
    fn eval_kwargs(&mut self, kwargs: &[(String, Expr)], env: &Env) -> Result<Vec<(String, Value)>, String> {
        let mut result = Vec::new();
        for (k, v) in kwargs {
            let val = self.eval(v, env)?;
            if k == "**" {
                // Spread dict into kwargs
                match val {
                    Value::Dict(map) => {
                        for (key, value) in map.borrow().iter() {
                            match key {
                                Value::Str(s) => result.push((s.clone(), value.clone())),
                                other => {
                                    return Err(
                                        self.err(&format!("**kwargs keys must be strings, got {}", other.type_name()))
                                    );
                                }
                            }
                        }
                    }
                    other => {
                        return Err(self.err(&format!("** requires a dict, got {}", other.type_name())));
                    }
                }
            } else {
                result.push((k.clone(), val));
            }
        }
        Ok(result)
    }

    fn call_builtin(&mut self, name: &str, args: Vec<Value>, env: &Env) -> Result<Value, String> {
        let _env = env;
        // Math module functions
        if let Some(math_fn) = name.strip_prefix("math.") {
            return self.call_math_fn(math_fn, args);
        }
        if let Some(os_fn) = name.strip_prefix("os.") {
            return self.call_os_fn(os_fn, args);
        }
        if let Some(path_fn) = name.strip_prefix("path.") {
            return self.call_path_fn(path_fn, args);
        }
        if let Some(f) = name.strip_prefix("string.") {
            return self.call_string_fn(f, args, env);
        }
        if let Some(f) = name.strip_prefix("list.") {
            return self.call_list_mod_fn(f, args, env);
        }
        if let Some(f) = name.strip_prefix("json.") {
            return self.call_json_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("toml.") {
            return self.call_toml_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("yaml.") {
            return self.call_yaml_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("sqlite.") {
            return self.call_sqlite_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("http.") {
            return self.call_http_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("re.") {
            return self.call_re_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("time.") {
            return self.call_time_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("random.") {
            return self.call_random_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("subprocess.") {
            return self.call_subprocess_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("argparse.") {
            return self.call_argparse_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("csv.") {
            return self.call_csv_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("datetime.") {
            return self.call_datetime_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("hashlib.") {
            return self.call_hashlib_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("test.") {
            return self.call_test_fn(f, args, env);
        }
        if let Some(f) = name.strip_prefix("logging.") {
            return self.call_logging_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("term.") {
            return self.call_term_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("platform.") {
            return self.call_platform_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("core.") {
            return self.call_core_fn(f, args);
        }
        if let Some(f) = name.strip_prefix("ffi.") {
            return self.call_ffi_builtin(f, args);
        }
        if let Some(f) = name.strip_prefix("socket.") {
            return self.call_socket_fn(f, args);
        }

        match name {
            "print" => {
                let mut parts = Vec::new();
                for v in args {
                    if let Value::Instance(ref inst) = v {
                        if let Some(func) = lookup_method(&inst.class, "__str__") {
                            match self.call_value(func, vec![v], vec![], env)? {
                                Value::Str(s) => {
                                    parts.push(s);
                                    continue;
                                }
                                other => {
                                    parts.push(other.to_string());
                                    continue;
                                }
                            }
                        }
                    }
                    parts.push(v.to_string());
                }
                println!("{}", parts.join(" "));
                Ok(Value::Nil)
            }
            "repr" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("repr() requires 1 argument"))?;
                Ok(Value::Str(repr(&v)))
            }
            "len" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("len() requires 1 argument"))?;
                match &v {
                    Value::Instance(inst) => {
                        if let Some(func) = lookup_method(&inst.class, "__len__") {
                            return self.call_value(func, vec![v], vec![], env);
                        }
                        Err(self.err(&format!("object of type '{}' has no len()", inst.class.name)))
                    }
                    Value::List(l) => Ok(Value::Int(l.borrow().len() as i64)),
                    Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                    Value::Dict(m) => Ok(Value::Int(m.borrow().keys.len() as i64)),
                    Value::Tuple(t) => Ok(Value::Int(t.len() as i64)),
                    other => Err(self.err(&format!("len() not supported for {}", other.type_name()))),
                }
            }
            "range" => match args.as_slice() {
                [Value::Int(end)] => Ok(Value::List(Rc::new(RefCell::new((0..*end).map(Value::Int).collect())))),
                [Value::Int(start), Value::Int(end)] => Ok(Value::List(Rc::new(RefCell::new(
                    (*start..*end).map(Value::Int).collect(),
                )))),
                [Value::Int(start), Value::Int(end), Value::Int(step)] => {
                    let mut v = Vec::new();
                    let mut i = *start;
                    if *step == 0 {
                        return Err(self.err("range() step cannot be 0"));
                    }
                    if *step > 0 {
                        while i < *end {
                            v.push(Value::Int(i));
                            i += step;
                        }
                    } else {
                        while i > *end {
                            v.push(Value::Int(i));
                            i += step;
                        }
                    }
                    Ok(Value::List(Rc::new(RefCell::new(v))))
                }
                _ => Err(self.err("range() takes 1–3 int arguments")),
            },
            "str" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("str() requires 1 argument"))?;
                // Check for __str__ method on instances
                if let Value::Instance(ref inst) = v {
                    if let Some(func) = lookup_method(&inst.class, "__str__") {
                        let result = self.call_value(func, vec![v], vec![], env)?;
                        return match result {
                            Value::Str(s) => Ok(Value::Str(s)),
                            other => Ok(Value::Str(other.to_string())),
                        };
                    }
                }
                Ok(Value::Str(v.to_string()))
            }
            "int" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("int() requires 1 argument"))?;
                Ok(Value::Int(self.coerce_to_int(&v)?))
            }
            "i8" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("i8() requires 1 argument"))?;
                Ok(Value::Int(wrap_signed(self.coerce_to_int(&v)?, 8)))
            }
            "u8" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("u8() requires 1 argument"))?;
                Ok(Value::Int(wrap_unsigned(self.coerce_to_int(&v)?, 8)))
            }
            "i16" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("i16() requires 1 argument"))?;
                Ok(Value::Int(wrap_signed(self.coerce_to_int(&v)?, 16)))
            }
            "u16" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("u16() requires 1 argument"))?;
                Ok(Value::Int(wrap_unsigned(self.coerce_to_int(&v)?, 16)))
            }
            "i32" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("i32() requires 1 argument"))?;
                Ok(Value::Int(wrap_signed(self.coerce_to_int(&v)?, 32)))
            }
            "u32" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("u32() requires 1 argument"))?;
                Ok(Value::Int(wrap_unsigned(self.coerce_to_int(&v)?, 32)))
            }
            "i64" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("i64() requires 1 argument"))?;
                Ok(Value::Int(self.coerce_to_int(&v)?))
            }
            "isize" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("isize() requires 1 argument"))?;
                Ok(Value::Int(wrap_signed(self.coerce_to_int(&v)?, COOL_POINTER_BITS)))
            }
            "usize" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("usize() requires 1 argument"))?;
                Ok(Value::Int(wrap_unsigned(self.coerce_to_int(&v)?, COOL_POINTER_BITS)))
            }
            "word_bits" => {
                if !args.is_empty() {
                    return Err(self.err("word_bits() takes no arguments"));
                }
                Ok(Value::Int(COOL_POINTER_BITS as i64))
            }
            "word_bytes" => {
                if !args.is_empty() {
                    return Err(self.err("word_bytes() takes no arguments"));
                }
                Ok(Value::Int(COOL_POINTER_BYTES))
            }
            "float" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("float() requires 1 argument"))?;
                match v {
                    Value::Float(f) => Ok(Value::Float(f)),
                    Value::Int(n) => Ok(Value::Float(n as f64)),
                    Value::Str(s) => s
                        .trim()
                        .parse::<f64>()
                        .map(Value::Float)
                        .map_err(|_| self.err(&format!("cannot convert \"{}\" to float", s))),
                    other => Err(self.err(&format!("cannot convert {} to float", other.type_name()))),
                }
            }
            "bool" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("bool() requires 1 argument"))?;
                Ok(Value::Bool(v.is_truthy()))
            }
            "type" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("type() requires 1 argument"))?;
                Ok(Value::Str(v.type_name().to_string()))
            }
            "abs" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("abs() requires 1 argument"))?;
                match v {
                    Value::Int(n) => Ok(Value::Int(n.abs())),
                    Value::Float(f) => Ok(Value::Float(f.abs())),
                    other => Err(self.err(&format!("abs() not supported for {}", other.type_name()))),
                }
            }
            "round" => {
                let ndigits = args
                    .get(1)
                    .and_then(|v| if let Value::Int(n) = v { Some(*n) } else { None })
                    .unwrap_or(0);
                match args.first() {
                    Some(Value::Int(n)) => Ok(Value::Int(*n)),
                    Some(Value::Float(f)) => {
                        if ndigits == 0 {
                            Ok(Value::Int(f.round() as i64))
                        } else {
                            let factor = 10f64.powi(ndigits as i32);
                            Ok(Value::Float((f * factor).round() / factor))
                        }
                    }
                    _ => Err(self.err("round() requires a number")),
                }
            }
            "min" => {
                if args.is_empty() {
                    return Err(self.err("min() requires at least 1 argument"));
                }
                let items = if args.len() == 1 {
                    match args.into_iter().next().unwrap() {
                        Value::List(l) => l.borrow().clone(),
                        v => vec![v],
                    }
                } else {
                    args
                };
                items
                    .into_iter()
                    .try_fold(None, |acc: Option<Value>, x| match acc {
                        None => Ok(Some(x)),
                        Some(cur) => {
                            let ord = compare_values(&x, &cur)?;
                            Ok(Some(if ord == std::cmp::Ordering::Less { x } else { cur }))
                        }
                    })
                    .map(|v| v.unwrap_or(Value::Nil))
            }
            "max" => {
                if args.is_empty() {
                    return Err(self.err("max() requires at least 1 argument"));
                }
                let items = if args.len() == 1 {
                    match args.into_iter().next().unwrap() {
                        Value::List(l) => l.borrow().clone(),
                        v => vec![v],
                    }
                } else {
                    args
                };
                items
                    .into_iter()
                    .try_fold(None, |acc: Option<Value>, x| match acc {
                        None => Ok(Some(x)),
                        Some(cur) => {
                            let ord = compare_values(&x, &cur)?;
                            Ok(Some(if ord == std::cmp::Ordering::Greater { x } else { cur }))
                        }
                    })
                    .map(|v| v.unwrap_or(Value::Nil))
            }
            "any" => {
                if args.is_empty() {
                    return Err(self.err("any() requires at least 1 argument"));
                }
                let items = if args.len() == 1 {
                    match args.into_iter().next().unwrap() {
                        Value::List(l) => l.borrow().clone(),
                        v => vec![v],
                    }
                } else {
                    args
                };
                for v in items {
                    if v.is_truthy() {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            "all" => {
                if args.is_empty() {
                    return Err(self.err("all() requires at least 1 argument"));
                }
                let items = if args.len() == 1 {
                    match args.into_iter().next().unwrap() {
                        Value::List(l) => l.borrow().clone(),
                        v => vec![v],
                    }
                } else {
                    args
                };
                for v in items {
                    if !v.is_truthy() {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            "sum" => {
                let items: Vec<Value> = match args.into_iter().next() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    Some(Value::Tuple(t)) => t.as_ref().clone(),
                    _ => return Err(self.err("sum() requires a list or tuple")),
                };
                items
                    .into_iter()
                    .try_fold(Value::Int(0), |acc, x| self.apply_binop(&BinOp::Add, acc, x))
            }
            "sorted" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    Some(Value::Str(s)) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
                    _ => return Err(self.err("sorted() requires an iterable")),
                };
                let mut v = lst;
                let mut err: Option<String> = None;
                v.sort_by(|a, b| match compare_values(a, b) {
                    Ok(o) => o,
                    Err(e) => {
                        err = Some(e);
                        std::cmp::Ordering::Equal
                    }
                });
                if let Some(e) = err {
                    return Err(e);
                }
                Ok(Value::List(Rc::new(RefCell::new(v))))
            }
            "reversed" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => return Err(self.err("reversed() requires a list")),
                };
                let mut v = lst;
                v.reverse();
                Ok(Value::List(Rc::new(RefCell::new(v))))
            }
            "enumerate" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => return Err(self.err("enumerate() requires a list")),
                };
                let pairs: Vec<Value> = lst
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| Value::List(Rc::new(RefCell::new(vec![Value::Int(i as i64), v]))))
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(pairs))))
            }
            "zip" => {
                if args.len() < 2 {
                    return Err(self.err("zip() requires at least 2 lists"));
                }
                let lists: Result<Vec<Vec<Value>>, String> = args
                    .into_iter()
                    .map(|a| match a {
                        Value::List(l) => Ok(l.borrow().clone()),
                        other => Err(self.err(&format!("zip() requires lists, got {}", other.type_name()))),
                    })
                    .collect();
                let lists = lists?;
                let len = lists.iter().map(|l| l.len()).min().unwrap_or(0);
                let result: Vec<Value> = (0..len)
                    .map(|i| Value::List(Rc::new(RefCell::new(lists.iter().map(|l| l[i].clone()).collect()))))
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(result))))
            }
            "map" => {
                let (func, lst) = match args.as_slice() {
                    [f, Value::List(_)] => (f.clone(), args[1].clone()),
                    _ => return Err(self.err("map() requires a function and a list")),
                };
                let lst = match lst {
                    Value::List(l) => l.borrow().clone(),
                    _ => unreachable!(),
                };
                let result: Result<Vec<Value>, String> = lst
                    .into_iter()
                    .map(|item| self.call_value(func.clone(), vec![item], vec![], _env))
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(result?))))
            }
            "filter" => {
                let (func, lst) = match args.as_slice() {
                    [_, Value::List(_)] => (args[0].clone(), args[1].clone()),
                    _ => return Err(self.err("filter() requires a function and a list")),
                };
                let lst = match lst {
                    Value::List(l) => l.borrow().clone(),
                    _ => unreachable!(),
                };
                let mut result = Vec::new();
                for item in lst {
                    let v = self.call_value(func.clone(), vec![item.clone()], vec![], _env)?;
                    if v.is_truthy() {
                        result.push(item);
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(result))))
            }
            "input" => {
                let prompt = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                if let Some(editor) = &mut self.readline_editor {
                    match editor.readline(&prompt) {
                        Ok(line) => {
                            let _ = editor.add_history_entry(line.as_str());
                            Ok(Value::Str(line))
                        }
                        Err(rustyline::error::ReadlineError::Eof) => Ok(Value::Str(String::new())),
                        Err(rustyline::error::ReadlineError::Interrupted) => {
                            std::process::exit(0);
                        }
                        Err(e) => Err(self.err(&format!("input() error: {}", e))),
                    }
                } else {
                    use std::io::Write;
                    print!("{}", prompt);
                    std::io::stdout().flush().ok();
                    let mut line = String::new();
                    std::io::stdin()
                        .read_line(&mut line)
                        .map_err(|e| self.err(&format!("input() error: {}", e)))?;
                    Ok(Value::Str(line.trim_end_matches('\n').to_string()))
                }
            }
            "set_completions" => {
                if let Some(Value::List(lst)) = args.first() {
                    let completions: Vec<String> = lst
                        .borrow()
                        .iter()
                        .filter_map(|v| if let Value::Str(s) = v { Some(s.clone()) } else { None })
                        .collect();
                    COMPLETIONS.with(|c| *c.borrow_mut() = completions);
                }
                Ok(Value::Nil)
            }
            "eval" => {
                let code = match args.into_iter().next() {
                    Some(Value::Str(s)) => s,
                    _ => return Err(self.err("eval() requires a string")),
                };
                // Only use the expression path if the entire input is consumed
                let expr_opt: Option<crate::ast::Expr> = {
                    let mut lex = crate::lexer::Lexer::new(&code);
                    if let Ok(toks) = lex.tokenize() {
                        let mut p = crate::parser::Parser::new(toks);
                        match p.parse_expr() {
                            Ok(e) if p.is_at_end() => Some(e),
                            _ => None,
                        }
                    } else {
                        None
                    }
                };
                if let Some(expr) = expr_opt {
                    self.eval(&expr, env)
                } else {
                    let mut lex2 = crate::lexer::Lexer::new(&code);
                    let tokens = lex2.tokenize().map_err(|e| self.err(&e))?;
                    let mut parser2 = crate::parser::Parser::new(tokens);
                    let prog = parser2.parse_program().map_err(|e| self.err(&e))?;
                    self.exec_block(&prog, env)?;
                    Ok(Value::Nil)
                }
            }
            "exit" => {
                let code = match args.first() {
                    Some(Value::Int(n)) => *n as i32,
                    _ => 0,
                };
                std::process::exit(code);
            }
            "runfile" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("runfile() requires a file path string")),
                };
                let extra_args: Vec<String> = match args.get(1) {
                    None => Vec::new(),
                    Some(Value::List(lst)) => lst.borrow().iter().map(|v| format!("{}", v)).collect(),
                    Some(Value::Tuple(items)) => items.iter().map(|v| format!("{}", v)).collect(),
                    Some(_) => return Err(self.err("runfile() 2nd argument must be a list or tuple of args")),
                };
                let full_path = if std::path::Path::new(&path).is_absolute() {
                    std::path::PathBuf::from(&path)
                } else {
                    self.source_dir.join(&path)
                };
                let source = std::fs::read_to_string(&full_path)
                    .map_err(|e| self.err(&format!("runfile: cannot read '{}': {}", full_path.display(), e)))?;
                let mut lexer = crate::lexer::Lexer::new(&source);
                let tokens = lexer.tokenize().map_err(|e| self.err(&e))?;
                let mut parser = crate::parser::Parser::new(tokens);
                let program = parser.parse_program().map_err(|e| self.err(&e))?;
                let old_dir = self.source_dir.clone();
                let old_script_path = std::env::var("COOL_SCRIPT_PATH").ok();
                let old_program_args = std::env::var("COOL_PROGRAM_ARGS").ok();
                if let Some(parent) = full_path.parent() {
                    self.source_dir = parent.to_path_buf();
                }
                std::env::set_var("COOL_SCRIPT_PATH", full_path.to_string_lossy().to_string());
                if !extra_args.is_empty() {
                    std::env::set_var("COOL_PROGRAM_ARGS", extra_args.join("\x1F"));
                } else {
                    std::env::remove_var("COOL_PROGRAM_ARGS");
                }
                let run_env = Env::new_global();
                let result = self.exec_block(&program, &run_env);
                self.source_dir = old_dir;
                if let Some(script_path) = old_script_path {
                    std::env::set_var("COOL_SCRIPT_PATH", script_path);
                } else {
                    std::env::remove_var("COOL_SCRIPT_PATH");
                }
                if let Some(program_args) = old_program_args {
                    std::env::set_var("COOL_PROGRAM_ARGS", program_args);
                } else {
                    std::env::remove_var("COOL_PROGRAM_ARGS");
                }
                match result {
                    Err(e) if e == "__raise__" => {
                        let v = self.pending_raise.take().unwrap_or(Value::Nil);
                        eprintln!("Error in {}: {}", full_path.display(), v);
                    }
                    Err(e) => eprintln!("Error in {}: {}", full_path.display(), e),
                    Ok(Signal::Raise(v)) => eprintln!("Unhandled exception in {}: {}", full_path.display(), v),
                    Ok(_) => {}
                }
                Ok(Value::Nil)
            }
            "super" => {
                // super() — find `self` in current env and return Super proxy
                match _env.get("self") {
                    Some(Value::Instance(inst)) => match &inst.class.parent {
                        Some(parent) => Ok(Value::Super {
                            instance: inst.clone(),
                            parent: parent.clone(),
                        }),
                        None => Err(self.err("super(): class has no parent")),
                    },
                    _ => Err(self.err("super() called outside of a method")),
                }
            }
            "open" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("open() requires a file path string")),
                };
                let mode = match args.get(1) {
                    Some(Value::Str(m)) => m.clone(),
                    None => "r".to_string(),
                    _ => return Err(self.err("open() mode must be a string")),
                };

                // Resolve relative paths
                let full_path = if std::path::Path::new(&path).is_absolute() {
                    path.clone()
                } else {
                    self.source_dir.join(&path).to_string_lossy().to_string()
                };

                let (content, write_buf) = if mode.contains('r') {
                    let text = std::fs::read_to_string(&full_path)
                        .map_err(|e| self.err(&format!("cannot open '{}': {}", path, e)))?;
                    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                    (lines, String::new())
                } else if mode.contains('a') {
                    let existing = std::fs::read_to_string(&full_path).unwrap_or_default();
                    (vec![], existing)
                } else {
                    (vec![], String::new())
                };

                let fh = FileHandle {
                    path: full_path,
                    mode,
                    content,
                    line_pos: 0,
                    write_buf: RefCell::new(write_buf),
                    closed: false,
                };
                Ok(Value::File(Rc::new(RefCell::new(fh))))
            }
            "list" => match args.into_iter().next() {
                None => Ok(Value::List(Rc::new(RefCell::new(vec![])))),
                Some(v) => {
                    let items = self.to_iterable(v)?;
                    Ok(Value::List(Rc::new(RefCell::new(items))))
                }
            },
            "tuple" => match args.into_iter().next() {
                None => Ok(Value::Tuple(Rc::new(vec![]))),
                Some(v) => {
                    let items = self.to_iterable(v)?;
                    Ok(Value::Tuple(Rc::new(items)))
                }
            },
            "dict" => Ok(Value::Dict(Rc::new(RefCell::new(IndexedMap::new())))),
            "set" => {
                // Sets are not natively supported; return a list of unique values
                match args.into_iter().next() {
                    None => Ok(Value::List(Rc::new(RefCell::new(vec![])))),
                    Some(v) => {
                        let items = self.to_iterable(v)?;
                        let mut unique: Vec<Value> = Vec::new();
                        for item in items {
                            if !unique.iter().any(|u| values_equal(u, &item)) {
                                unique.push(item);
                            }
                        }
                        Ok(Value::List(Rc::new(RefCell::new(unique))))
                    }
                }
            }
            "isinstance" => {
                let (val, cls_name) = match args.as_slice() {
                    [v, Value::Str(s)] => (v.clone(), s.clone()),
                    [v, Value::Class(c)] => (v.clone(), c.name.clone()),
                    _ => {
                        return Err(self.err("isinstance() requires a value and a class or type name"));
                    }
                };
                let result = match &val {
                    Value::Instance(inst) => is_instance_of(&inst.class, &cls_name),
                    v => v.type_name() == cls_name.as_str(),
                };
                Ok(Value::Bool(result))
            }
            "hasattr" => {
                let (val, attr) = match args.as_slice() {
                    [v, Value::Str(s)] => (v.clone(), s.clone()),
                    _ => return Err(self.err("hasattr() requires a value and an attribute name")),
                };
                let result = match &val {
                    Value::Instance(inst) => {
                        inst.fields.borrow().contains_key(&attr) || lookup_method(&inst.class, &attr).is_some()
                    }
                    Value::Dict(m) => m.borrow().contains(&Value::Str(attr)),
                    _ => false,
                };
                Ok(Value::Bool(result))
            }
            "getattr" => {
                let (val, attr) = match args.as_slice() {
                    [v, Value::Str(s)] => (v.clone(), s.clone()),
                    _ => return Err(self.err("getattr() requires a value and an attribute name")),
                };
                match &val {
                    Value::Instance(inst) => {
                        if let Some(v) = inst.fields.borrow().get(&attr) {
                            return Ok(v.clone());
                        }
                        if let Some(m) = lookup_method(&inst.class, &attr) {
                            return Ok(m);
                        }
                        let default = args.get(2).cloned().unwrap_or(Value::Nil);
                        Ok(default)
                    }
                    _ => {
                        let default = args.get(2).cloned().unwrap_or(Value::Nil);
                        Ok(default)
                    }
                }
            }
            "asm"
            | "malloc"
            | "free"
            | "read_byte"
            | "write_byte"
            | "read_i8"
            | "write_i8"
            | "read_u8"
            | "write_u8"
            | "read_i16"
            | "write_i16"
            | "read_u16"
            | "write_u16"
            | "read_i32"
            | "write_i32"
            | "read_u32"
            | "write_u32"
            | "read_i64"
            | "write_i64"
            | "read_f64"
            | "write_f64"
            | "read_byte_volatile"
            | "write_byte_volatile"
            | "read_i8_volatile"
            | "write_i8_volatile"
            | "read_u8_volatile"
            | "write_u8_volatile"
            | "read_i16_volatile"
            | "write_i16_volatile"
            | "read_u16_volatile"
            | "write_u16_volatile"
            | "read_i32_volatile"
            | "write_i32_volatile"
            | "read_u32_volatile"
            | "write_u32_volatile"
            | "read_i64_volatile"
            | "write_i64_volatile"
            | "read_f64_volatile"
            | "write_f64_volatile"
            | "read_str"
            | "write_str"
            | "outb"
            | "inb"
            | "write_serial_byte" => Err(self.err(&format!(
                "'{}' is only supported in the LLVM backend — compile with `cool build`",
                name
            ))),
            _ => Err(self.err(&format!("unknown builtin '{}'", name))),
        }
    }

    fn coerce_to_int(&self, value: &Value) -> Result<i64, String> {
        match value {
            Value::Int(n) => Ok(*n),
            Value::Float(f) => Ok(*f as i64),
            Value::Str(s) => s
                .trim()
                .parse::<i64>()
                .map_err(|_| self.err(&format!("cannot convert \"{}\" to int", s))),
            Value::Bool(b) => Ok(if *b { 1 } else { 0 }),
            other => Err(self.err(&format!("cannot convert {} to int", other.type_name()))),
        }
    }

    fn call_math_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        // Functions that don't need a numeric first arg
        match name {
            "isnan" => {
                let n = as_float_arg(&args, 0, "math.isnan")?;
                return Ok(Value::Bool(n.is_nan()));
            }
            "isinf" => {
                let n = as_float_arg(&args, 0, "math.isinf")?;
                return Ok(Value::Bool(n.is_infinite()));
            }
            "isfinite" => {
                let n = as_float_arg(&args, 0, "math.isfinite")?;
                return Ok(Value::Bool(n.is_finite()));
            }
            "gcd" => {
                let a = match args.get(0) {
                    Some(Value::Int(i)) => *i,
                    _ => return Err(self.err("math.gcd() requires integers")),
                };
                let b = match args.get(1) {
                    Some(Value::Int(i)) => *i,
                    _ => return Err(self.err("math.gcd() requires 2 integers")),
                };
                fn gcd(a: i64, b: i64) -> i64 {
                    if b == 0 {
                        a.abs()
                    } else {
                        gcd(b, a % b)
                    }
                }
                return Ok(Value::Int(gcd(a, b)));
            }
            "lcm" => {
                let a = match args.get(0) {
                    Some(Value::Int(i)) => *i,
                    _ => return Err(self.err("math.lcm() requires integers")),
                };
                let b = match args.get(1) {
                    Some(Value::Int(i)) => *i,
                    _ => return Err(self.err("math.lcm() requires 2 integers")),
                };
                fn gcd(a: i64, b: i64) -> i64 {
                    if b == 0 {
                        a.abs()
                    } else {
                        gcd(b, a % b)
                    }
                }
                return Ok(Value::Int(if a == 0 || b == 0 { 0 } else { (a * b).abs() / gcd(a, b) }));
            }
            "factorial" => {
                let n = match args.get(0) {
                    Some(Value::Int(i)) => *i,
                    _ => return Err(self.err("math.factorial() requires an integer")),
                };
                if n < 0 {
                    return Err(self.err("math.factorial() requires a non-negative integer"));
                }
                let result: i64 = (1..=n).product();
                return Ok(Value::Int(result));
            }
            "hypot" => {
                let a = as_float_arg(&args, 0, "math.hypot")?;
                let b = as_float_arg(&args, 1, "math.hypot")?;
                return Ok(Value::Float(a.hypot(b)));
            }
            "atan2" => {
                let a = as_float_arg(&args, 0, "math.atan2")?;
                let b = as_float_arg(&args, 1, "math.atan2")?;
                return Ok(Value::Float(a.atan2(b)));
            }
            "pow" => {
                let a = as_float_arg(&args, 0, "math.pow")?;
                let b = as_float_arg(&args, 1, "math.pow")?;
                return Ok(Value::Float(a.powf(b)));
            }
            _ => {}
        }
        let n = as_float_arg(&args, 0, &format!("math.{}", name))?;
        let result = match name {
            "sqrt" => n.sqrt(),
            "floor" => {
                return Ok(Value::Int(n.floor() as i64));
            }
            "ceil" => {
                return Ok(Value::Int(n.ceil() as i64));
            }
            "trunc" => {
                return Ok(Value::Int(n.trunc() as i64));
            }
            "round" => {
                let ndigits = args
                    .get(1)
                    .and_then(|v| if let Value::Int(n) = v { Some(*n) } else { None })
                    .unwrap_or(0);
                if ndigits == 0 {
                    return Ok(Value::Int(n.round() as i64));
                }
                let factor = 10f64.powi(ndigits as i32);
                return Ok(Value::Float((n * factor).round() / factor));
            }
            "abs" => {
                return match args.first() {
                    Some(Value::Int(v)) => Ok(Value::Int(v.abs())),
                    _ => Ok(Value::Float(n.abs())),
                };
            }
            "log" => {
                if args.len() >= 2 {
                    let base = as_float_arg(&args, 1, "math.log")?;
                    return Ok(Value::Float(n.log(base)));
                }
                n.ln()
            }
            "log2" => n.log2(),
            "log10" => n.log10(),
            "exp" => n.exp(),
            "exp2" => n.exp2(),
            "sin" => n.sin(),
            "cos" => n.cos(),
            "tan" => n.tan(),
            "asin" => n.asin(),
            "acos" => n.acos(),
            "atan" => n.atan(),
            "sinh" => n.sinh(),
            "cosh" => n.cosh(),
            "tanh" => n.tanh(),
            "degrees" => n.to_degrees(),
            "radians" => n.to_radians(),
            _ => return Err(self.err(&format!("math has no function '{}'", name))),
        };
        Ok(Value::Float(result))
    }

    fn call_os_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "getcwd" => {
                let path = std::env::current_dir().map_err(|e| self.err(&format!("os.getcwd() error: {}", e)))?;
                Ok(Value::Str(path.to_string_lossy().to_string()))
            }
            "listdir" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    None => ".".to_string(),
                    _ => return Err(self.err("os.listdir() requires a path string")),
                };
                let entries: Result<Vec<Value>, String> = std::fs::read_dir(&path)
                    .map_err(|e| self.err(&format!("os.listdir() error: {}", e)))?
                    .map(|entry| {
                        entry
                            .map(|e| Value::Str(e.file_name().to_string_lossy().to_string()))
                            .map_err(|e| self.err(&format!("os.listdir() error: {}", e)))
                    })
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(entries?))))
            }
            "exists" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.exists() requires a path string")),
                };
                Ok(Value::Bool(std::path::Path::new(&path).exists()))
            }
            "isdir" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.isdir() requires a path string")),
                };
                Ok(Value::Bool(std::path::Path::new(&path).is_dir()))
            }
            "getenv" => {
                let name = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.getenv() requires an env var name")),
                };
                Ok(match std::env::var(&name) {
                    Ok(v) => Value::Str(v),
                    Err(_) => Value::Nil,
                })
            }
            "join" | "path" => {
                if args.is_empty() {
                    return Ok(Value::Str(String::new()));
                }
                let parts: Result<Vec<String>, String> = args
                    .iter()
                    .map(|a| match a {
                        Value::Str(s) => Ok(s.clone()),
                        _ => Err(self.err("os.join() requires string arguments")),
                    })
                    .collect();
                let parts = parts?;
                let path: std::path::PathBuf = parts.iter().collect();
                Ok(Value::Str(path.to_string_lossy().to_string()))
            }
            "mkdir" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.mkdir() requires a path string")),
                };
                std::fs::create_dir_all(&path).map_err(|e| self.err(&format!("os.mkdir() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "remove" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.remove() requires a path string")),
                };
                std::fs::remove_file(&path).map_err(|e| self.err(&format!("os.remove() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "rename" => {
                let from = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.rename() requires 2 path strings")),
                };
                let to = match args.get(1) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.rename() requires 2 path strings")),
                };
                std::fs::rename(&from, &to).map_err(|e| self.err(&format!("os.rename() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "popen" => {
                let cmd = match args.into_iter().next() {
                    Some(Value::Str(s)) => s,
                    _ => return Err(self.err("os.popen() requires a command string")),
                };
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .output()
                    .map_err(|e| self.err(&format!("os.popen() error: {}", e)))?;
                Ok(Value::Str(String::from_utf8_lossy(&output.stdout).to_string()))
            }
            _ => Err(self.err(&format!("os has no function '{}'", name))),
        }
    }

    fn subprocess_result_value(&self, result: SubprocessResult) -> Value {
        let mut map = IndexedMap::new();
        map.set(
            Value::Str("code".to_string()),
            result.code.map(Value::Int).unwrap_or(Value::Nil),
        );
        map.set(Value::Str("stdout".to_string()), Value::Str(result.stdout));
        map.set(Value::Str("stderr".to_string()), Value::Str(result.stderr));
        map.set(Value::Str("timed_out".to_string()), Value::Bool(result.timed_out));
        map.set(
            Value::Str("ok".to_string()),
            Value::Bool(!result.timed_out && result.code == Some(0)),
        );
        Value::Dict(Rc::new(RefCell::new(map)))
    }

    fn subprocess_timeout_arg(&self, args: &[Value], idx: usize, name: &str) -> Result<Option<f64>, String> {
        match args.get(idx) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Int(i)) => Ok(Some(*i as f64)),
            Some(Value::Float(f)) => Ok(Some(*f)),
            _ => Err(self.err(&format!("subprocess.{}() timeout must be a number", name))),
        }
    }

    fn subprocess_command_arg(&self, args: &[Value], name: &str) -> Result<String, String> {
        match args.first() {
            Some(Value::Str(s)) => Ok(s.clone()),
            _ => Err(self.err(&format!("subprocess.{}() requires a command string", name))),
        }
    }

    fn call_subprocess_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() || args.len() > 2 {
            return Err(self.err(&format!("subprocess.{}() takes 1 or 2 arguments", name)));
        }
        let command = self.subprocess_command_arg(&args, name)?;
        let timeout = self.subprocess_timeout_arg(&args, 1, name)?;
        let result = run_shell_command(&command, timeout)
            .map_err(|e| self.err(&format!("subprocess.{}() error: {}", name, e)))?;

        match name {
            "run" => Ok(self.subprocess_result_value(result)),
            "call" => Ok(result.code.map(Value::Int).unwrap_or(Value::Nil)),
            "check_output" => {
                if result.timed_out {
                    return Err(self.err("subprocess.check_output() timed out"));
                }
                if result.code != Some(0) {
                    let code = result.code.map(|n| n.to_string()).unwrap_or_else(|| "nil".to_string());
                    let detail = if result.stderr.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", result.stderr.trim_end())
                    };
                    return Err(self.err(&format!(
                        "subprocess.check_output() exited with code {}{}",
                        code, detail
                    )));
                }
                Ok(Value::Str(result.stdout))
            }
            _ => Err(self.err(&format!("subprocess has no function '{}'", name))),
        }
    }

    fn value_to_arg_data(&self, value: &Value) -> Result<ArgData, String> {
        match value {
            Value::Int(n) => Ok(ArgData::Int(*n)),
            Value::Float(f) => Ok(ArgData::Float(*f)),
            Value::Str(s) => Ok(ArgData::Str(s.clone())),
            Value::Bool(b) => Ok(ArgData::Bool(*b)),
            Value::Nil => Ok(ArgData::Nil),
            Value::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.value_to_arg_data(item)?);
                }
                Ok(ArgData::List(out))
            }
            Value::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.value_to_arg_data(item)?);
                }
                Ok(ArgData::Tuple(out))
            }
            Value::Dict(map) => {
                let map = map.borrow();
                let mut out = Vec::with_capacity(map.keys.len());
                for (key, value) in map.keys.iter().zip(map.vals.iter()) {
                    out.push((self.value_to_arg_data(key)?, self.value_to_arg_data(value)?));
                }
                Ok(ArgData::Dict(out))
            }
            other => Err(self.err(&format!(
                "argparse only accepts scalar/list/tuple/dict values, got {}",
                other.type_name()
            ))),
        }
    }

    fn arg_data_to_value(data: &ArgData) -> Value {
        match data {
            ArgData::Int(n) => Value::Int(*n),
            ArgData::Float(f) => Value::Float(*f),
            ArgData::Str(s) => Value::Str(s.clone()),
            ArgData::Bool(b) => Value::Bool(*b),
            ArgData::Nil => Value::Nil,
            ArgData::List(items) => Value::List(Rc::new(RefCell::new(
                items.iter().map(Self::arg_data_to_value).collect(),
            ))),
            ArgData::Tuple(items) => Value::Tuple(Rc::new(items.iter().map(Self::arg_data_to_value).collect())),
            ArgData::Dict(items) => {
                let mut out = IndexedMap::new();
                for (key, value) in items {
                    out.set(Self::arg_data_to_value(key), Self::arg_data_to_value(value));
                }
                Value::Dict(Rc::new(RefCell::new(out)))
            }
        }
    }

    fn argparse_argv_arg(&self, value: Option<&Value>) -> Result<Vec<String>, String> {
        match value {
            None | Some(Value::Nil) => Ok(argparse_runtime::current_process_argv().into_iter().skip(1).collect()),
            Some(Value::List(items)) => items
                .borrow()
                .iter()
                .map(|item| match item {
                    Value::Str(s) => Ok(s.clone()),
                    other => Err(self.err(&format!(
                        "argparse.parse() argv items must be strings, got {}",
                        other.type_name()
                    ))),
                })
                .collect(),
            Some(Value::Tuple(items)) => items
                .iter()
                .map(|item| match item {
                    Value::Str(s) => Ok(s.clone()),
                    other => Err(self.err(&format!(
                        "argparse.parse() argv items must be strings, got {}",
                        other.type_name()
                    ))),
                })
                .collect(),
            Some(other) => Err(self.err(&format!(
                "argparse.parse() argv must be a list or tuple of strings, got {}",
                other.type_name()
            ))),
        }
    }

    fn call_argparse_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "parse" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("argparse.parse() takes a spec dict and optional argv list"));
                }
                let spec = self.value_to_arg_data(&args[0])?;
                let argv = self.argparse_argv_arg(args.get(1))?;
                let parsed = argparse_runtime::parse(&spec, &argv, Some(&argparse_runtime::default_prog_name()))
                    .map_err(|e| self.err(&e))?;
                Ok(Self::arg_data_to_value(&parsed))
            }
            "help" => {
                if args.len() != 1 {
                    return Err(self.err("argparse.help() takes exactly one spec dict"));
                }
                let spec = self.value_to_arg_data(&args[0])?;
                let rendered = argparse_runtime::help(&spec, Some(&argparse_runtime::default_prog_name()))
                    .map_err(|e| self.err(&e))?;
                Ok(Value::Str(rendered))
            }
            _ => Err(self.err(&format!("argparse has no function '{}'", name))),
        }
    }

    fn test_message_arg(&self, args: &[Value], idx: usize, fname: &str) -> Result<Option<String>, String> {
        match args.get(idx) {
            None => Ok(None),
            Some(Value::Nil) => Ok(None),
            Some(value) => {
                if args.len() > idx + 1 {
                    return Err(self.err(&format!("test.{}() takes at most {} arguments", fname, idx + 1)));
                }
                Ok(Some(format!("{}", value)))
            }
        }
    }

    fn test_default_message(&self, message: Option<String>, default: impl FnOnce() -> String) -> String {
        message.unwrap_or_else(default)
    }

    fn test_raise_assertion(&mut self, message: String) -> Result<Value, String> {
        self.pending_raise = Some(Value::Str(format!("AssertionError: {}", message)));
        Err("__raise__".to_string())
    }

    fn test_args_list(&self, value: Option<&Value>) -> Result<Vec<Value>, String> {
        match value {
            None | Some(Value::Nil) => Ok(Vec::new()),
            Some(Value::List(items)) => Ok(items.borrow().clone()),
            Some(Value::Tuple(items)) => Ok(items.iter().cloned().collect()),
            Some(other) => Err(self.err(&format!(
                "test.raises() args must be a list or tuple, got {}",
                other.type_name()
            ))),
        }
    }

    fn test_expected_exc_name(&self, value: Option<&Value>) -> Result<Option<String>, String> {
        match value {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Str(name)) => Ok(Some(name.clone())),
            Some(Value::Class(cls)) => Ok(Some(cls.name.clone())),
            Some(other) => Err(self.err(&format!(
                "test.raises() expected exception must be a string/class or nil, got {}",
                other.type_name()
            ))),
        }
    }

    fn call_test_fn(&mut self, name: &str, args: Vec<Value>, env: &Env) -> Result<Value, String> {
        match name {
            "equal" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(self.err("test.equal() takes actual, expected, and optional message"));
                }
                if !values_equal(&args[0], &args[1]) {
                    let message = self.test_default_message(self.test_message_arg(&args, 2, name)?, || {
                        format!("expected {} == {}", repr(&args[0]), repr(&args[1]))
                    });
                    return self.test_raise_assertion(message);
                }
                Ok(Value::Nil)
            }
            "not_equal" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(self.err("test.not_equal() takes actual, expected, and optional message"));
                }
                if values_equal(&args[0], &args[1]) {
                    let message = self.test_default_message(self.test_message_arg(&args, 2, name)?, || {
                        format!("expected {} != {}", repr(&args[0]), repr(&args[1]))
                    });
                    return self.test_raise_assertion(message);
                }
                Ok(Value::Nil)
            }
            "true" | "truthy" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.truthy() takes a value and optional message"));
                }
                if !args[0].is_truthy() {
                    let message = self.test_default_message(self.test_message_arg(&args, 1, name)?, || {
                        "expected truthy value".into()
                    });
                    return self.test_raise_assertion(message);
                }
                Ok(Value::Nil)
            }
            "false" | "falsey" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.falsey() takes a value and optional message"));
                }
                if args[0].is_truthy() {
                    let message = self.test_default_message(self.test_message_arg(&args, 1, name)?, || {
                        "expected falsey value".into()
                    });
                    return self.test_raise_assertion(message);
                }
                Ok(Value::Nil)
            }
            "nil" | "is_nil" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.is_nil() takes a value and optional message"));
                }
                if !matches!(args[0], Value::Nil) {
                    let message = self.test_default_message(self.test_message_arg(&args, 1, name)?, || {
                        format!("expected nil, got {}", repr(&args[0]))
                    });
                    return self.test_raise_assertion(message);
                }
                Ok(Value::Nil)
            }
            "not_nil" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.not_nil() takes a value and optional message"));
                }
                if matches!(args[0], Value::Nil) {
                    let message = self.test_default_message(self.test_message_arg(&args, 1, name)?, || {
                        "expected non-nil value".into()
                    });
                    return self.test_raise_assertion(message);
                }
                Ok(Value::Nil)
            }
            "fail" => {
                if args.len() > 1 {
                    return Err(self.err("test.fail() takes at most one message argument"));
                }
                let message = args
                    .first()
                    .map(|value| format!("{}", value))
                    .unwrap_or_else(|| "test.fail() called".to_string());
                self.test_raise_assertion(message)
            }
            "raises" => {
                if args.is_empty() || args.len() > 3 {
                    return Err(
                        self.err("test.raises() takes a callable, optional args list, and optional expected exception")
                    );
                }
                let callable = args[0].clone();
                let call_args = self.test_args_list(args.get(1))?;
                let expected = self.test_expected_exc_name(args.get(2))?;
                match self.call_value(callable, call_args, vec![], env) {
                    Ok(_) => {
                        self.test_raise_assertion("expected exception, but call returned successfully".to_string())
                    }
                    Err(err) if err == "__raise__" => {
                        let exc = self.pending_raise.take().unwrap_or(Value::Nil);
                        if let Some(expected_name) = expected {
                            let matches = match &exc {
                                Value::Instance(inst) => is_instance_of(&inst.class, &expected_name),
                                Value::Str(s) => {
                                    s == &expected_name
                                        || s.starts_with(&format!("{}:", expected_name))
                                        || expected_name == "Exception"
                                }
                                other => other.type_name() == expected_name,
                            };
                            if !matches {
                                return self.test_raise_assertion(format!(
                                    "expected exception {}, got {}",
                                    expected_name,
                                    repr(&exc)
                                ));
                            }
                        }
                        Ok(exc)
                    }
                    Err(err) => self.test_raise_assertion(format!("expected exception, got runtime error: {}", err)),
                }
            }
            _ => Err(self.err(&format!("test has no function '{}'", name))),
        }
    }

    fn csv_rows_to_value(&self, rows: Vec<Vec<String>>) -> Value {
        Value::List(Rc::new(RefCell::new(
            rows.into_iter()
                .map(|row| {
                    Value::List(Rc::new(RefCell::new(
                        row.into_iter().map(Value::Str).collect::<Vec<_>>(),
                    )))
                })
                .collect(),
        )))
    }

    fn csv_dicts_to_value(&self, rows: Vec<Vec<(String, String)>>) -> Value {
        Value::List(Rc::new(RefCell::new(
            rows.into_iter()
                .map(|row| {
                    let mut map = IndexedMap::new();
                    for (key, value) in row {
                        map.set(Value::Str(key), Value::Str(value));
                    }
                    Value::Dict(Rc::new(RefCell::new(map)))
                })
                .collect(),
        )))
    }

    fn csv_write_rows_arg(&self, value: &Value) -> Result<Vec<Vec<String>>, String> {
        let rows: Vec<Value> = match value {
            Value::List(items) => items.borrow().clone(),
            Value::Tuple(items) => items.iter().cloned().collect(),
            other => {
                return Err(self.err(&format!(
                    "csv.write() rows must be a list or tuple, got {}",
                    other.type_name()
                )))
            }
        };

        let Some(first) = rows.first() else {
            return Ok(Vec::new());
        };

        if matches!(first, Value::Dict(_)) {
            let first_map = match first {
                Value::Dict(map) => map.borrow(),
                _ => unreachable!(),
            };
            let header_keys = first_map.keys.clone();
            let header_row: Vec<String> = header_keys.iter().map(|key| format!("{}", key)).collect();
            drop(first_map);

            let mut out = Vec::with_capacity(rows.len() + 1);
            out.push(header_row);
            for row in rows {
                let map = match row {
                    Value::Dict(map) => map,
                    other => {
                        return Err(self.err(&format!(
                            "csv.write() rows must all be dicts when the first row is a dict, got {}",
                            other.type_name()
                        )))
                    }
                };
                let map = map.borrow();
                let mut cols = Vec::with_capacity(header_keys.len());
                for key in &header_keys {
                    cols.push(map.get(key).map(|value| format!("{}", value)).unwrap_or_default());
                }
                out.push(cols);
            }
            Ok(out)
        } else {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                match row {
                    Value::List(items) => {
                        out.push(items.borrow().iter().map(|value| format!("{}", value)).collect());
                    }
                    Value::Tuple(items) => {
                        out.push(items.iter().map(|value| format!("{}", value)).collect());
                    }
                    other => {
                        return Err(self.err(&format!(
                            "csv.write() rows must contain only lists, tuples, or dicts, got {}",
                            other.type_name()
                        )))
                    }
                }
            }
            Ok(out)
        }
    }

    fn call_csv_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "rows" => {
                if args.len() != 1 {
                    return Err(self.err("csv.rows() takes exactly one string argument"));
                }
                let text = match &args[0] {
                    Value::Str(s) => s,
                    other => return Err(self.err(&format!("csv.rows() requires a string, got {}", other.type_name()))),
                };
                let rows = csv_runtime::parse_rows(text).map_err(|e| self.err(&e))?;
                Ok(self.csv_rows_to_value(rows))
            }
            "dicts" => {
                if args.len() != 1 {
                    return Err(self.err("csv.dicts() takes exactly one string argument"));
                }
                let text = match &args[0] {
                    Value::Str(s) => s,
                    other => return Err(self.err(&format!("csv.dicts() requires a string, got {}", other.type_name()))),
                };
                let rows = csv_runtime::parse_dicts(text).map_err(|e| self.err(&e))?;
                Ok(self.csv_dicts_to_value(rows))
            }
            "write" => {
                if args.len() != 1 {
                    return Err(self.err("csv.write() takes exactly one rows argument"));
                }
                Ok(Value::Str(csv_runtime::write_rows(&self.csv_write_rows_arg(&args[0])?)))
            }
            _ => Err(self.err(&format!("csv has no function '{}'", name))),
        }
    }

    fn datetime_parts_to_value(&self, parts: DateTimeParts) -> Value {
        let mut map = IndexedMap::new();
        map.set(Value::Str("year".to_string()), Value::Int(parts.year));
        map.set(Value::Str("month".to_string()), Value::Int(parts.month));
        map.set(Value::Str("day".to_string()), Value::Int(parts.day));
        map.set(Value::Str("hour".to_string()), Value::Int(parts.hour));
        map.set(Value::Str("minute".to_string()), Value::Int(parts.minute));
        map.set(Value::Str("second".to_string()), Value::Int(parts.second));
        map.set(Value::Str("weekday".to_string()), Value::Int(parts.weekday));
        map.set(Value::Str("yearday".to_string()), Value::Int(parts.yearday));
        Value::Dict(Rc::new(RefCell::new(map)))
    }

    fn datetime_number_arg(&self, value: &Value, context: &str) -> Result<f64, String> {
        match value {
            Value::Int(n) => Ok(*n as f64),
            Value::Float(f) if f.is_finite() => Ok(*f),
            Value::Float(_) => Err(self.err(&format!("{context} must be a finite number"))),
            other => Err(self.err(&format!("{context} must be a number, got {}", other.type_name()))),
        }
    }

    fn datetime_format_arg<'a>(&self, value: Option<&'a Value>, name: &str) -> Result<Option<&'a str>, String> {
        match value {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::Str(s)) => Ok(Some(s.as_str())),
            Some(other) => Err(self.err(&format!(
                "datetime.{}() format must be a string or nil, got {}",
                name,
                other.type_name()
            ))),
        }
    }

    fn call_datetime_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "now" => {
                if !args.is_empty() {
                    return Err(self.err("datetime.now() takes no arguments"));
                }
                Ok(Value::Float(datetime_runtime::now()))
            }
            "format" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("datetime.format() takes a timestamp and optional format string"));
                }
                let timestamp = self.datetime_number_arg(&args[0], "datetime.format() timestamp")?;
                let rendered = datetime_runtime::format(timestamp, self.datetime_format_arg(args.get(1), name)?)
                    .map_err(|e| self.err(&e))?;
                Ok(Value::Str(rendered))
            }
            "parse" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("datetime.parse() takes a text string and optional format string"));
                }
                let text = match &args[0] {
                    Value::Str(s) => s.as_str(),
                    other => {
                        return Err(self.err(&format!(
                            "datetime.parse() text must be a string, got {}",
                            other.type_name()
                        )))
                    }
                };
                let timestamp = datetime_runtime::parse(text, self.datetime_format_arg(args.get(1), name)?)
                    .map_err(|e| self.err(&e))?;
                Ok(Value::Float(timestamp))
            }
            "parts" => {
                if args.len() != 1 {
                    return Err(self.err("datetime.parts() takes exactly one timestamp argument"));
                }
                let timestamp = self.datetime_number_arg(&args[0], "datetime.parts() timestamp")?;
                let parts = datetime_runtime::parts(timestamp).map_err(|e| self.err(&e))?;
                Ok(self.datetime_parts_to_value(parts))
            }
            "add_seconds" => {
                if args.len() != 2 {
                    return Err(self.err("datetime.add_seconds() takes a timestamp and seconds value"));
                }
                let timestamp = self.datetime_number_arg(&args[0], "datetime.add_seconds() timestamp")?;
                let seconds = self.datetime_number_arg(&args[1], "datetime.add_seconds() seconds")?;
                let shifted = datetime_runtime::add_seconds(timestamp, seconds).map_err(|e| self.err(&e))?;
                Ok(Value::Float(shifted))
            }
            "diff_seconds" => {
                if args.len() != 2 {
                    return Err(self.err("datetime.diff_seconds() takes two timestamp values"));
                }
                let left = self.datetime_number_arg(&args[0], "datetime.diff_seconds() left timestamp")?;
                let right = self.datetime_number_arg(&args[1], "datetime.diff_seconds() right timestamp")?;
                let diff = datetime_runtime::diff_seconds(left, right).map_err(|e| self.err(&e))?;
                Ok(Value::Float(diff))
            }
            _ => Err(self.err(&format!("datetime has no function '{}'", name))),
        }
    }

    fn hashlib_text_arg<'a>(&self, value: &'a Value, context: &str) -> Result<&'a str, String> {
        match value {
            Value::Str(s) => Ok(s.as_str()),
            other => Err(self.err(&format!("{context} requires a string, got {}", other.type_name()))),
        }
    }

    fn call_hashlib_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "md5" | "sha1" | "sha256" => {
                if args.len() != 1 {
                    return Err(self.err(&format!("hashlib.{}() takes exactly one text argument", name)));
                }
                let text = self.hashlib_text_arg(&args[0], &format!("hashlib.{}()", name))?;
                let digest = match name {
                    "md5" => hashlib_runtime::md5_hex(text),
                    "sha1" => hashlib_runtime::sha1_hex(text),
                    "sha256" => hashlib_runtime::sha256_hex(text),
                    _ => unreachable!(),
                };
                Ok(Value::Str(digest))
            }
            "digest" => {
                if args.len() != 2 {
                    return Err(self.err("hashlib.digest() takes an algorithm name and text argument"));
                }
                let algo = self.hashlib_text_arg(&args[0], "hashlib.digest() algorithm")?;
                let text = self.hashlib_text_arg(&args[1], "hashlib.digest() text")?;
                let digest = hashlib_runtime::digest_hex(algo, text).map_err(|e| self.err(&e))?;
                Ok(Value::Str(digest))
            }
            _ => Err(self.err(&format!("hashlib has no function '{}'", name))),
        }
    }

    fn value_to_log_data(&self, value: &Value) -> Result<LogData, String> {
        match value {
            Value::Int(n) => Ok(LogData::Int(*n)),
            Value::Float(f) => Ok(LogData::Float(*f)),
            Value::Str(s) => Ok(LogData::Str(s.clone())),
            Value::Bool(b) => Ok(LogData::Bool(*b)),
            Value::Nil => Ok(LogData::Nil),
            Value::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.value_to_log_data(item)?);
                }
                Ok(LogData::List(out))
            }
            Value::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.value_to_log_data(item)?);
                }
                Ok(LogData::Tuple(out))
            }
            Value::Dict(map) => {
                let map = map.borrow();
                let mut out = Vec::with_capacity(map.keys.len());
                for (key, value) in map.keys.iter().zip(map.vals.iter()) {
                    out.push((self.value_to_log_data(key)?, self.value_to_log_data(value)?));
                }
                Ok(LogData::Dict(out))
            }
            other => Err(self.err(&format!(
                "logging only accepts scalar/list/tuple/dict values, got {}",
                other.type_name()
            ))),
        }
    }

    fn logging_name_arg(&self, args: &[Value], idx: usize, fname: &str) -> Result<Option<String>, String> {
        if args.len() > idx + 1 {
            return Err(self.err(&format!("logging.{}() takes at most {} arguments", fname, idx + 1)));
        }
        Ok(args.get(idx).map(|value| format!("{}", value)))
    }

    fn call_logging_fn(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "basic_config" => {
                if args.len() > 1 {
                    return Err(self.err("logging.basic_config() takes at most one config dict"));
                }
                let config = match args.first() {
                    None => None,
                    Some(value) => Some(self.value_to_log_data(value)?),
                };
                logging_runtime::configure(&mut self.logging_state, config.as_ref()).map_err(|e| self.err(&e))?;
                Ok(Value::Nil)
            }
            "log" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(self.err("logging.log() takes a level string, message, and optional logger name"));
                }
                let level = match &args[0] {
                    Value::Str(s) => LogLevel::parse(s).map_err(|e| self.err(&e))?,
                    other => {
                        return Err(self.err(&format!(
                            "logging.log() level must be a string, got {}",
                            other.type_name()
                        )))
                    }
                };
                let message = format!("{}", args[1]);
                let logger_name = self.logging_name_arg(&args, 2, name)?;
                logging_runtime::emit(&mut self.logging_state, level, &message, logger_name.as_deref())
                    .map_err(|e| self.err(&e))?;
                Ok(Value::Nil)
            }
            "debug" | "info" | "warning" | "warn" | "error" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err(&format!("logging.{}() takes a message and optional logger name", name)));
                }
                let level = match name {
                    "debug" => LogLevel::Debug,
                    "info" => LogLevel::Info,
                    "warning" | "warn" => LogLevel::Warning,
                    "error" => LogLevel::Error,
                    _ => unreachable!(),
                };
                let message = format!("{}", args[0]);
                let logger_name = self.logging_name_arg(&args, 1, name)?;
                logging_runtime::emit(&mut self.logging_state, level, &message, logger_name.as_deref())
                    .map_err(|e| self.err(&e))?;
                Ok(Value::Nil)
            }
            _ => Err(self.err(&format!("logging has no function '{}'", name))),
        }
    }

    fn normalize_path_string(path: &str) -> String {
        use std::path::{Component, Path, PathBuf};

        let p = Path::new(path);
        let is_abs = p.is_absolute();
        let mut parts: Vec<String> = Vec::new();
        for component in p.components() {
            match component {
                Component::RootDir => {}
                Component::CurDir => {}
                Component::ParentDir => {
                    if let Some(last) = parts.last() {
                        if last != ".." {
                            parts.pop();
                        } else if !is_abs {
                            parts.push("..".to_string());
                        }
                    } else if !is_abs {
                        parts.push("..".to_string());
                    }
                }
                Component::Normal(seg) => parts.push(seg.to_string_lossy().to_string()),
                Component::Prefix(prefix) => parts.push(prefix.as_os_str().to_string_lossy().to_string()),
            }
        }

        let mut out = if is_abs { PathBuf::from("/") } else { PathBuf::new() };
        for part in parts {
            out.push(part);
        }
        let rendered = out.to_string_lossy().to_string();
        if rendered.is_empty() {
            if is_abs {
                "/".to_string()
            } else {
                ".".to_string()
            }
        } else {
            rendered
        }
    }

    fn call_path_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        fn req_path_arg<F>(args: &[Value], idx: usize, err: F) -> Result<String, String>
        where
            F: Fn() -> String,
        {
            match args.get(idx) {
                Some(Value::Str(s)) => Ok(s.clone()),
                _ => Err(err()),
            }
        }

        match name {
            "join" => {
                let parts: Result<Vec<String>, String> = args
                    .iter()
                    .map(|a| match a {
                        Value::Str(s) => Ok(s.clone()),
                        _ => Err(self.err("path.join() requires string arguments")),
                    })
                    .collect();
                let mut path = std::path::PathBuf::new();
                for part in parts? {
                    path.push(part);
                }
                Ok(Value::Str(path.to_string_lossy().to_string()))
            }
            "basename" => {
                let path = req_path_arg(&args, 0, || self.err("path.basename() requires a path string"))?;
                let out = std::path::Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                Ok(Value::Str(out))
            }
            "dirname" => {
                let path = req_path_arg(&args, 0, || self.err("path.dirname() requires a path string"))?;
                let p = std::path::Path::new(&path);
                let out = if path == "/" {
                    "/".to_string()
                } else {
                    p.parent().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
                };
                Ok(Value::Str(out))
            }
            "ext" => {
                let path = req_path_arg(&args, 0, || self.err("path.ext() requires a path string"))?;
                let out = std::path::Path::new(&path)
                    .extension()
                    .map(|s| format!(".{}", s.to_string_lossy()))
                    .unwrap_or_default();
                Ok(Value::Str(out))
            }
            "stem" => {
                let path = req_path_arg(&args, 0, || self.err("path.stem() requires a path string"))?;
                let out = std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                Ok(Value::Str(out))
            }
            "split" => {
                let path = req_path_arg(&args, 0, || self.err("path.split() requires a path string"))?;
                let dir = if path == "/" {
                    "/".to_string()
                } else {
                    std::path::Path::new(&path)
                        .parent()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                };
                let base = std::path::Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                Ok(Value::List(Rc::new(RefCell::new(vec![
                    Value::Str(dir),
                    Value::Str(base),
                ]))))
            }
            "normalize" => {
                let path = req_path_arg(&args, 0, || self.err("path.normalize() requires a path string"))?;
                Ok(Value::Str(Self::normalize_path_string(&path)))
            }
            "exists" => {
                let path = req_path_arg(&args, 0, || self.err("path.exists() requires a path string"))?;
                Ok(Value::Bool(std::path::Path::new(&path).exists()))
            }
            "isabs" => {
                let path = req_path_arg(&args, 0, || self.err("path.isabs() requires a path string"))?;
                Ok(Value::Bool(std::path::Path::new(&path).is_absolute()))
            }
            _ => Err(self.err(&format!("path has no function '{}'", name))),
        }
    }

    fn call_platform_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(self.err(&format!("platform.{name}() takes no arguments")));
        }

        let value = match name {
            "os" => Value::Str(std::env::consts::OS.to_string()),
            "arch" => Value::Str(std::env::consts::ARCH.to_string()),
            "family" => Value::Str(std::env::consts::FAMILY.to_string()),
            "runtime" => Value::Str("interpreter".to_string()),
            "exe_ext" => Value::Str(std::env::consts::EXE_EXTENSION.to_string()),
            "shared_lib_ext" => Value::Str(
                if cfg!(target_os = "windows") {
                    "dll"
                } else if cfg!(target_os = "macos") {
                    "dylib"
                } else {
                    "so"
                }
                .to_string(),
            ),
            "path_sep" => Value::Str(std::path::MAIN_SEPARATOR.to_string()),
            "newline" => Value::Str(if cfg!(windows) { "\r\n" } else { "\n" }.to_string()),
            "is_windows" => Value::Bool(cfg!(windows)),
            "is_unix" => Value::Bool(cfg!(unix)),
            "has_ffi" => Value::Bool(true),
            "has_raw_memory" => Value::Bool(false),
            "has_extern" => Value::Bool(false),
            "has_inline_asm" => Value::Bool(false),
            _ => return Err(self.err(&format!("platform has no function '{}'", name))),
        };
        Ok(value)
    }

    fn call_core_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let req_int = |idx: usize, context: &str| -> Result<i64, String> {
            let value = args
                .get(idx)
                .ok_or_else(|| self.err(&format!("{context}() missing argument")))?;
            self.coerce_to_int(value)
        };

        match name {
            "word_bits" => {
                if !args.is_empty() {
                    return Err(self.err("core.word_bits() takes no arguments"));
                }
                Ok(Value::Int(core_runtime::word_bits()))
            }
            "word_bytes" => {
                if !args.is_empty() {
                    return Err(self.err("core.word_bytes() takes no arguments"));
                }
                Ok(Value::Int(core_runtime::word_bytes()))
            }
            "page_size" => {
                if !args.is_empty() {
                    return Err(self.err("core.page_size() takes no arguments"));
                }
                Ok(Value::Int(core_runtime::page_size()))
            }
            "page_align_down" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_align_down() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::page_align_down(req_int(
                    0,
                    "core.page_align_down",
                )?)))
            }
            "page_align_up" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_align_up() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::page_align_up(req_int(
                    0,
                    "core.page_align_up",
                )?)))
            }
            "page_offset" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_offset() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::page_offset(req_int(0, "core.page_offset")?)))
            }
            "page_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_index() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::page_index(req_int(0, "core.page_index")?)))
            }
            "page_count" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_count() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::page_count(req_int(0, "core.page_count")?)))
            }
            "pt_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pt_index() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::pt_index(req_int(0, "core.pt_index")?)))
            }
            "pd_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pd_index() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::pd_index(req_int(0, "core.pd_index")?)))
            }
            "pdpt_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pdpt_index() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::pdpt_index(req_int(0, "core.pdpt_index")?)))
            }
            "pml4_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pml4_index() takes exactly one argument"));
                }
                Ok(Value::Int(core_runtime::pml4_index(req_int(0, "core.pml4_index")?)))
            }
            "alloc" | "free" | "set_allocator" | "clear_allocator" => Err(self.err(&format!(
                "core.{name}() is only supported in the LLVM backend — compile with `cool build`"
            ))),
            _ => Err(self.err(&format!("core has no function '{}'", name))),
        }
    }

    // ── string module ─────────────────────────────────────────────────────

    fn call_string_fn(&mut self, name: &str, args: Vec<Value>, _env: &Env) -> Result<Value, String> {
        match name {
            "split" => {
                let s = req_str_arg(&args, 0, "string.split")?;
                let sep = match args.get(1) {
                    Some(Value::Str(sep)) => Some(sep.clone()),
                    None => None,
                    _ => return Err(self.err("string.split() separator must be a string")),
                };
                let parts: Vec<Value> = match sep {
                    Some(sep) => s.split(&*sep).map(|p| Value::Str(p.to_string())).collect(),
                    None => s.split_whitespace().map(|p| Value::Str(p.to_string())).collect(),
                };
                Ok(Value::List(Rc::new(RefCell::new(parts))))
            }
            "join" => {
                let sep = req_str_arg(&args, 0, "string.join")?;
                let lst = match args.get(1) {
                    Some(Value::List(l)) => l.clone(),
                    _ => return Err(self.err("string.join() requires a list as 2nd argument")),
                };
                let parts: Vec<String> = lst.borrow().iter().map(|v| v.to_string()).collect();
                Ok(Value::Str(parts.join(&sep)))
            }
            "strip" => {
                let s = req_str_arg(&args, 0, "string.strip")?;
                Ok(Value::Str(s.trim().to_string()))
            }
            "lstrip" => {
                let s = req_str_arg(&args, 0, "string.lstrip")?;
                Ok(Value::Str(s.trim_start().to_string()))
            }
            "rstrip" => {
                let s = req_str_arg(&args, 0, "string.rstrip")?;
                Ok(Value::Str(s.trim_end().to_string()))
            }
            "upper" => {
                let s = req_str_arg(&args, 0, "string.upper")?;
                Ok(Value::Str(s.to_uppercase()))
            }
            "lower" => {
                let s = req_str_arg(&args, 0, "string.lower")?;
                Ok(Value::Str(s.to_lowercase()))
            }
            "title" => {
                let s = req_str_arg(&args, 0, "string.title")?;
                Ok(Value::Str(
                    s.split_whitespace()
                        .map(|w| {
                            let mut c = w.chars();
                            c.next()
                                .map(|f| f.to_uppercase().to_string() + c.as_str())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                ))
            }
            "capitalize" => {
                let s = req_str_arg(&args, 0, "string.capitalize")?;
                let mut c = s.chars();
                Ok(Value::Str(
                    c.next()
                        .map(|f| f.to_uppercase().to_string() + c.as_str())
                        .unwrap_or_default(),
                ))
            }
            "replace" => {
                let s = req_str_arg(&args, 0, "string.replace")?;
                let old = req_str_arg(&args, 1, "string.replace")?;
                let new = req_str_arg(&args, 2, "string.replace")?;
                Ok(Value::Str(s.replace(&*old, &*new)))
            }
            "startswith" => {
                let s = req_str_arg(&args, 0, "string.startswith")?;
                let pre = req_str_arg(&args, 1, "string.startswith")?;
                Ok(Value::Bool(s.starts_with(&*pre)))
            }
            "endswith" => {
                let s = req_str_arg(&args, 0, "string.endswith")?;
                let suf = req_str_arg(&args, 1, "string.endswith")?;
                Ok(Value::Bool(s.ends_with(&*suf)))
            }
            "find" => {
                let s = req_str_arg(&args, 0, "string.find")?;
                let sub = req_str_arg(&args, 1, "string.find")?;
                Ok(Value::Int(s.find(&*sub).map(|i| i as i64).unwrap_or(-1)))
            }
            "count" => {
                let s = req_str_arg(&args, 0, "string.count")?;
                let sub = req_str_arg(&args, 1, "string.count")?;
                Ok(Value::Int(s.matches(&*sub).count() as i64))
            }
            "format" => {
                // string.format(template, *args) — same as template.format(args)
                let s = req_str_arg(&args, 0, "string.format")?;
                let rest = args.into_iter().skip(1).collect::<Vec<_>>();
                let mut result = s.clone();
                for v in rest {
                    if let Some(pos) = result.find("{}") {
                        result.replace_range(pos..pos + 2, &v.to_string());
                    }
                }
                Ok(Value::Str(result))
            }
            _ => Err(self.err(&format!("string has no function '{}'", name))),
        }
    }

    // ── list module ───────────────────────────────────────────────────────

    fn call_list_mod_fn(&mut self, name: &str, args: Vec<Value>, env: &Env) -> Result<Value, String> {
        match name {
            "sort" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l,
                    _ => return Err(self.err("list.sort() requires a list")),
                };
                let mut v = lst.borrow().clone();
                v.sort_by(|a, b| compare_values(a, b).unwrap_or(std::cmp::Ordering::Equal));
                Ok(Value::List(Rc::new(RefCell::new(v))))
            }
            "reverse" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l,
                    _ => return Err(self.err("list.reverse() requires a list")),
                };
                let mut v = lst.borrow().clone();
                v.reverse();
                Ok(Value::List(Rc::new(RefCell::new(v))))
            }
            "map" => {
                if args.len() < 2 {
                    return Err(self.err("list.map() requires (fn, list)"));
                }
                let func = args[0].clone();
                let lst = match &args[1] {
                    Value::List(l) => l.clone(),
                    _ => return Err(self.err("list.map() 2nd argument must be a list")),
                };
                let items = lst.borrow().clone();
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.call_value(func.clone(), vec![item], vec![], env)?);
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            "filter" => {
                if args.len() < 2 {
                    return Err(self.err("list.filter() requires (fn, list)"));
                }
                let func = args[0].clone();
                let lst = match &args[1] {
                    Value::List(l) => l.clone(),
                    _ => return Err(self.err("list.filter() 2nd argument must be a list")),
                };
                let items = lst.borrow().clone();
                let mut out = Vec::new();
                for item in items {
                    let keep = self.call_value(func.clone(), vec![item.clone()], vec![], env)?;
                    if keep.is_truthy() {
                        out.push(item);
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            "reduce" => {
                if args.len() < 2 {
                    return Err(self.err("list.reduce() requires (fn, list[, initial])"));
                }
                let func = args[0].clone();
                let lst = match &args[1] {
                    Value::List(l) => l.clone(),
                    _ => return Err(self.err("list.reduce() 2nd argument must be a list")),
                };
                let items = lst.borrow().clone();
                let mut iter = items.into_iter();
                let mut acc = if args.len() >= 3 {
                    args[2].clone()
                } else {
                    iter.next()
                        .ok_or_else(|| self.err("list.reduce() called on empty list with no initial value"))?
                };
                for item in iter {
                    acc = self.call_value(func.clone(), vec![acc, item], vec![], env)?;
                }
                Ok(acc)
            }
            "flatten" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l,
                    _ => return Err(self.err("list.flatten() requires a list")),
                };
                let mut out = Vec::new();
                for item in lst.borrow().iter() {
                    match item {
                        Value::List(inner) => out.extend(inner.borrow().clone()),
                        other => out.push(other.clone()),
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            "unique" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l,
                    _ => return Err(self.err("list.unique() requires a list")),
                };
                let mut out: Vec<Value> = Vec::new();
                for item in lst.borrow().iter() {
                    if !out.iter().any(|x| values_equal(x, item)) {
                        out.push(item.clone());
                    }
                }
                Ok(Value::List(Rc::new(RefCell::new(out))))
            }
            _ => Err(self.err(&format!("list module has no function '{}'", name))),
        }
    }

    // ── json module ───────────────────────────────────────────────────────

    fn call_json_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "loads" => {
                let s = req_str_arg(&args, 0, "json.loads")?;
                json_parse(&s).map_err(|e| self.err(&format!("json.loads() error: {}", e)))
            }
            "dumps" => {
                let v = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("json.dumps() requires 1 argument"))?;
                Ok(Value::Str(json_dumps(&v)))
            }
            _ => Err(self.err(&format!("json has no function '{}'", name))),
        }
    }

    fn value_to_toml_data(&self, value: &Value) -> Result<TomlData, String> {
        match value {
            Value::Int(n) => Ok(TomlData::Int(*n)),
            Value::Float(f) if f.is_finite() => Ok(TomlData::Float(*f)),
            Value::Float(_) => Err(self.err("toml.dumps() does not support NaN or infinite floats")),
            Value::Str(s) => Ok(TomlData::Str(s.clone())),
            Value::Bool(b) => Ok(TomlData::Bool(*b)),
            Value::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.value_to_toml_data(item)?);
                }
                Ok(TomlData::List(out))
            }
            Value::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.value_to_toml_data(item)?);
                }
                Ok(TomlData::List(out))
            }
            Value::Dict(map) => {
                let map = map.borrow();
                let mut out = Vec::with_capacity(map.keys.len());
                for (key, value) in map.keys.iter().zip(map.vals.iter()) {
                    let key = match key {
                        Value::Str(s) => s.clone(),
                        other => {
                            return Err(self.err(&format!(
                                "toml.dumps() dict keys must be strings, got {}",
                                other.type_name()
                            )))
                        }
                    };
                    out.push((key, self.value_to_toml_data(value)?));
                }
                Ok(TomlData::Dict(out))
            }
            other => Err(self.err(&format!(
                "toml.dumps() only supports ints/floats/strings/bools/lists/tuples/dicts, got {}",
                other.type_name()
            ))),
        }
    }

    fn toml_data_to_value(data: &TomlData) -> Value {
        match data {
            TomlData::Int(n) => Value::Int(*n),
            TomlData::Float(f) => Value::Float(*f),
            TomlData::Str(s) => Value::Str(s.clone()),
            TomlData::Bool(b) => Value::Bool(*b),
            TomlData::List(items) => Value::List(Rc::new(RefCell::new(
                items.iter().map(Self::toml_data_to_value).collect(),
            ))),
            TomlData::Dict(items) => {
                let mut out = IndexedMap::new();
                for (key, value) in items {
                    out.set(Value::Str(key.clone()), Self::toml_data_to_value(value));
                }
                Value::Dict(Rc::new(RefCell::new(out)))
            }
        }
    }

    fn call_toml_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "loads" => {
                let s = req_str_arg(&args, 0, "toml.loads")?;
                Ok(Self::toml_data_to_value(
                    &toml_runtime::loads(&s).map_err(|e| self.err(&e))?,
                ))
            }
            "dumps" => {
                let value = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("toml.dumps() requires 1 argument"))?;
                Ok(Value::Str(
                    toml_runtime::dumps(&self.value_to_toml_data(&value)?).map_err(|e| self.err(&e))?,
                ))
            }
            _ => Err(self.err(&format!("toml has no function '{}'", name))),
        }
    }

    fn value_to_yaml_data(&self, value: &Value) -> Result<YamlData, String> {
        match value {
            Value::Nil => Ok(YamlData::Nil),
            Value::Int(n) => Ok(YamlData::Int(*n)),
            Value::Float(f) if f.is_finite() => Ok(YamlData::Float(*f)),
            Value::Float(_) => Err(self.err("yaml.dumps() does not support NaN or infinite floats")),
            Value::Str(s) => Ok(YamlData::Str(s.clone())),
            Value::Bool(b) => Ok(YamlData::Bool(*b)),
            Value::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.value_to_yaml_data(item)?);
                }
                Ok(YamlData::List(out))
            }
            Value::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.value_to_yaml_data(item)?);
                }
                Ok(YamlData::List(out))
            }
            Value::Dict(map) => {
                let map = map.borrow();
                let mut out = Vec::with_capacity(map.keys.len());
                for (key, value) in map.keys.iter().zip(map.vals.iter()) {
                    let key = match key {
                        Value::Str(s) => s.clone(),
                        other => {
                            return Err(self.err(&format!(
                                "yaml.dumps() dict keys must be strings, got {}",
                                other.type_name()
                            )))
                        }
                    };
                    out.push((key, self.value_to_yaml_data(value)?));
                }
                Ok(YamlData::Dict(out))
            }
            other => Err(self.err(&format!(
                "yaml.dumps() only supports nil/ints/floats/strings/bools/lists/tuples/dicts, got {}",
                other.type_name()
            ))),
        }
    }

    fn yaml_data_to_value(data: &YamlData) -> Value {
        match data {
            YamlData::Nil => Value::Nil,
            YamlData::Int(n) => Value::Int(*n),
            YamlData::Float(f) => Value::Float(*f),
            YamlData::Str(s) => Value::Str(s.clone()),
            YamlData::Bool(b) => Value::Bool(*b),
            YamlData::List(items) => Value::List(Rc::new(RefCell::new(
                items.iter().map(Self::yaml_data_to_value).collect(),
            ))),
            YamlData::Dict(items) => {
                let mut out = IndexedMap::new();
                for (key, value) in items {
                    out.set(Value::Str(key.clone()), Self::yaml_data_to_value(value));
                }
                Value::Dict(Rc::new(RefCell::new(out)))
            }
        }
    }

    fn call_yaml_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "loads" => {
                let s = req_str_arg(&args, 0, "yaml.loads")?;
                Ok(Self::yaml_data_to_value(
                    &yaml_runtime::loads(&s).map_err(|e| self.err(&e))?,
                ))
            }
            "dumps" => {
                let value = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.err("yaml.dumps() requires 1 argument"))?;
                Ok(Value::Str(
                    yaml_runtime::dumps(&self.value_to_yaml_data(&value)?).map_err(|e| self.err(&e))?,
                ))
            }
            _ => Err(self.err(&format!("yaml has no function '{}'", name))),
        }
    }

    fn value_to_sql_data(&self, value: &Value) -> Result<SqlData, String> {
        match value {
            Value::Nil => Ok(SqlData::Nil),
            Value::Int(n) => Ok(SqlData::Int(*n)),
            Value::Float(f) if f.is_finite() => Ok(SqlData::Float(*f)),
            Value::Float(_) => Err(self.err("sqlite parameters do not support NaN or infinite floats")),
            Value::Str(s) => Ok(SqlData::Str(s.clone())),
            Value::Bool(b) => Ok(SqlData::Bool(*b)),
            other => Err(self.err(&format!(
                "sqlite parameters only support nil/int/float/str/bool, got {}",
                other.type_name()
            ))),
        }
    }

    fn sqlite_data_to_value(data: &SqlData) -> Value {
        match data {
            SqlData::Nil => Value::Nil,
            SqlData::Int(n) => Value::Int(*n),
            SqlData::Float(f) => Value::Float(*f),
            SqlData::Str(s) => Value::Str(s.clone()),
            SqlData::Bool(b) => Value::Bool(*b),
        }
    }

    fn sqlite_params_arg(&self, value: Option<&Value>) -> Result<Vec<SqlData>, String> {
        match value {
            None | Some(Value::Nil) => Ok(Vec::new()),
            Some(Value::List(items)) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.value_to_sql_data(item)?);
                }
                Ok(out)
            }
            Some(Value::Tuple(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.value_to_sql_data(item)?);
                }
                Ok(out)
            }
            Some(other) => Err(self.err(&format!(
                "sqlite params must be a list, tuple, or nil, got {}",
                other.type_name()
            ))),
        }
    }

    fn sqlite_rows_to_value(rows: Vec<Vec<(String, SqlData)>>) -> Value {
        let rows = rows
            .into_iter()
            .map(|row| {
                let mut dict = IndexedMap::new();
                for (key, value) in row {
                    dict.set(Value::Str(key), Self::sqlite_data_to_value(&value));
                }
                Value::Dict(Rc::new(RefCell::new(dict)))
            })
            .collect();
        Value::List(Rc::new(RefCell::new(rows)))
    }

    fn call_sqlite_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let path = req_str_arg(&args, 0, &format!("sqlite.{}", name))?;
        let sql = req_str_arg(&args, 1, &format!("sqlite.{}", name))?;
        let params = self.sqlite_params_arg(args.get(2))?;
        match name {
            "execute" => Ok(Value::Int(
                sqlite_runtime::execute(&path, &sql, &params).map_err(|e| self.err(&e))?,
            )),
            "query" => Ok(Self::sqlite_rows_to_value(
                sqlite_runtime::query(&path, &sql, &params).map_err(|e| self.err(&e))?,
            )),
            "scalar" => Ok(Self::sqlite_data_to_value(
                &sqlite_runtime::scalar(&path, &sql, &params).map_err(|e| self.err(&e))?,
            )),
            _ => Err(self.err(&format!("sqlite has no function '{}'", name))),
        }
    }

    fn http_headers_arg(&self, value: Option<&Value>, context: &str) -> Result<Vec<String>, String> {
        match value {
            None | Some(Value::Nil) => Ok(Vec::new()),
            Some(Value::List(items)) => items
                .borrow()
                .iter()
                .map(|item| match item {
                    Value::Str(s) => Ok(s.clone()),
                    other => Err(self.err(&format!(
                        "{context} headers must contain only strings, got {}",
                        other.type_name()
                    ))),
                })
                .collect(),
            Some(Value::Tuple(items)) => items
                .iter()
                .map(|item| match item {
                    Value::Str(s) => Ok(s.clone()),
                    other => Err(self.err(&format!(
                        "{context} headers must contain only strings, got {}",
                        other.type_name()
                    ))),
                })
                .collect(),
            Some(other) => Err(self.err(&format!(
                "{context} headers must be a list, tuple, or nil, got {}",
                other.type_name()
            ))),
        }
    }

    fn call_http_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "get" => {
                let url = req_str_arg(&args, 0, "http.get")?;
                let headers = self.http_headers_arg(args.get(1), "http.get()")?;
                Ok(Value::Str(http_runtime::get(&url, &headers).map_err(|e| self.err(&e))?))
            }
            "post" => {
                let url = req_str_arg(&args, 0, "http.post")?;
                let data = req_str_arg(&args, 1, "http.post")?;
                let headers = self.http_headers_arg(args.get(2), "http.post()")?;
                Ok(Value::Str(
                    http_runtime::post(&url, &data, &headers).map_err(|e| self.err(&e))?,
                ))
            }
            "head" => {
                let url = req_str_arg(&args, 0, "http.head")?;
                let headers = self.http_headers_arg(args.get(1), "http.head()")?;
                Ok(Value::Str(
                    http_runtime::head(&url, &headers).map_err(|e| self.err(&e))?,
                ))
            }
            "getjson" => {
                let url = req_str_arg(&args, 0, "http.getjson")?;
                let headers = self.http_headers_arg(args.get(1), "http.getjson()")?;
                let body = http_runtime::getjson(&url, &headers).map_err(|e| self.err(&e))?;
                json_parse(&body).map_err(|e| self.err(&format!("http.getjson() invalid JSON: {}", e)))
            }
            _ => Err(self.err(&format!("http has no function '{}'", name))),
        }
    }

    // ── socket module ────────────────────────────────────────────────────

    fn call_socket_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "connect" => {
                let host = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("socket.connect() requires a host string")),
                };
                let port = match args.get(1) {
                    Some(Value::Int(p)) => *p as u16,
                    _ => return Err(self.err("socket.connect() requires an integer port")),
                };
                let addr = format!("{host}:{port}");
                let stream = std::net::TcpStream::connect(&addr)
                    .map_err(|e| self.err(&format!("socket.connect() error: {e}")))?;
                Ok(Value::Socket(Rc::new(RefCell::new(SocketHandle {
                    kind: SocketKind::Stream(stream),
                    closed: false,
                    peer: addr,
                }))))
            }
            "listen" => {
                let host = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("socket.listen() requires a host string")),
                };
                let port = match args.get(1) {
                    Some(Value::Int(p)) => *p as u16,
                    _ => return Err(self.err("socket.listen() requires an integer port")),
                };
                let addr = format!("{host}:{port}");
                let listener =
                    std::net::TcpListener::bind(&addr).map_err(|e| self.err(&format!("socket.listen() error: {e}")))?;
                Ok(Value::Socket(Rc::new(RefCell::new(SocketHandle {
                    kind: SocketKind::Listener(listener),
                    closed: false,
                    peer: addr,
                }))))
            }
            other => Err(self.err(&format!("socket has no function '{other}'"))),
        }
    }

    fn socket_method(&self, obj: Value, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let sh_rc = match obj {
            Value::Socket(s) => s,
            _ => unreachable!(),
        };
        match method {
            "__enter__" => Ok(Value::Socket(sh_rc)),
            "__exit__" => {
                sh_rc.borrow_mut().closed = true;
                Ok(Value::Nil)
            }
            "send" => {
                let data = match args.into_iter().next() {
                    Some(Value::Str(s)) => s,
                    Some(v) => v.to_string(),
                    None => return Err(self.err("send() requires 1 argument")),
                };
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("send() on closed socket"));
                }
                match &mut sh.kind {
                    SocketKind::Stream(stream) => {
                        use std::io::Write as IoWrite;
                        let bytes = data.as_bytes();
                        stream
                            .write_all(bytes)
                            .map_err(|e| self.err(&format!("socket.send() error: {e}")))?;
                        Ok(Value::Int(bytes.len() as i64))
                    }
                    SocketKind::Listener(_) => Err(self.err("send() on server socket")),
                }
            }
            "recv" => {
                let size = match args.into_iter().next() {
                    Some(Value::Int(n)) => n as usize,
                    Some(_) => return Err(self.err("recv() requires an integer size")),
                    None => 4096,
                };
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("recv() on closed socket"));
                }
                match &mut sh.kind {
                    SocketKind::Stream(stream) => {
                        use std::io::Read;
                        let mut buf = vec![0u8; size];
                        let n = stream
                            .read(&mut buf)
                            .map_err(|e| self.err(&format!("socket.recv() error: {e}")))?;
                        Ok(Value::Str(String::from_utf8_lossy(&buf[..n]).to_string()))
                    }
                    SocketKind::Listener(_) => Err(self.err("recv() on server socket")),
                }
            }
            "readline" => {
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("readline() on closed socket"));
                }
                match &mut sh.kind {
                    SocketKind::Stream(stream) => {
                        use std::io::Read;
                        let mut line = String::new();
                        let mut byte = [0u8; 1];
                        loop {
                            let n = stream
                                .read(&mut byte)
                                .map_err(|e| self.err(&format!("socket.readline() error: {e}")))?;
                            if n == 0 {
                                break;
                            }
                            line.push(byte[0] as char);
                            if byte[0] == b'\n' {
                                break;
                            }
                        }
                        Ok(Value::Str(line))
                    }
                    SocketKind::Listener(_) => Err(self.err("readline() on server socket")),
                }
            }
            "accept" => {
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("accept() on closed socket"));
                }
                match &mut sh.kind {
                    SocketKind::Listener(listener) => {
                        let (stream, addr) = listener
                            .accept()
                            .map_err(|e| self.err(&format!("socket.accept() error: {e}")))?;
                        let peer = addr.to_string();
                        Ok(Value::Socket(Rc::new(RefCell::new(SocketHandle {
                            kind: SocketKind::Stream(stream),
                            closed: false,
                            peer,
                        }))))
                    }
                    SocketKind::Stream(_) => Err(self.err("accept() on client socket")),
                }
            }
            "close" => {
                sh_rc.borrow_mut().closed = true;
                Ok(Value::Nil)
            }
            other => Err(self.err(&format!("socket has no method '{other}'"))),
        }
    }

    // ── re module ─────────────────────────────────────────────────────────

    fn call_re_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let pattern = req_str_arg(&args, 0, &format!("re.{}", name))?;
        let text = req_str_arg(&args, 1, &format!("re.{}", name))?;
        let re = Regex::new(&pattern).map_err(|e| self.err(&format!("re.{}() invalid pattern: {}", name, e)))?;
        match name {
            "match" => {
                // Anchored at start
                let anchored = format!("^(?:{})", pattern);
                let re2 = Regex::new(&anchored).map_err(|e| self.err(&format!("re.match() invalid pattern: {}", e)))?;
                match re2.find(&text) {
                    Some(m) => Ok(Value::Str(m.as_str().to_string())),
                    None => Ok(Value::Nil),
                }
            }
            "fullmatch" => {
                let anchored = format!("^(?:{})$", pattern);
                let re2 =
                    Regex::new(&anchored).map_err(|e| self.err(&format!("re.fullmatch() invalid pattern: {}", e)))?;
                match re2.find(&text) {
                    Some(m) => Ok(Value::Str(m.as_str().to_string())),
                    None => Ok(Value::Nil),
                }
            }
            "search" => match re.find(&text) {
                Some(m) => Ok(Value::Str(m.as_str().to_string())),
                None => Ok(Value::Nil),
            },
            "findall" => {
                let matches: Vec<Value> = re
                    .find_iter(&text)
                    .map(|m| Value::Str(m.as_str().to_string()))
                    .collect();
                Ok(Value::List(Rc::new(RefCell::new(matches))))
            }
            "sub" => {
                let repl = req_str_arg(&args, 2, "re.sub")?;
                Ok(Value::Str(re.replace_all(&text, repl.as_str()).to_string()))
            }
            "split" => {
                let parts: Vec<Value> = re.split(&text).map(|s| Value::Str(s.to_string())).collect();
                Ok(Value::List(Rc::new(RefCell::new(parts))))
            }
            _ => Err(self.err(&format!("re has no function '{}'", name))),
        }
    }

    // ── time module ───────────────────────────────────────────────────────

    fn call_time_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "time" => {
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                Ok(Value::Float(t))
            }
            "monotonic" => {
                // Use process start as epoch; we just return seconds as float
                static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                let start = START.get_or_init(std::time::Instant::now);
                Ok(Value::Float(start.elapsed().as_secs_f64()))
            }
            "sleep" => {
                let secs = as_float_arg(&args, 0, "time.sleep")?.max(0.0);
                std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                Ok(Value::Nil)
            }
            _ => Err(self.err(&format!("time has no function '{}'", name))),
        }
    }

    // ── random module ─────────────────────────────────────────────────────

    fn xorshift64(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    fn call_random_fn(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "random" => {
                let bits = self.xorshift64();
                Ok(Value::Float((bits >> 11) as f64 / (1u64 << 53) as f64))
            }
            "randint" => {
                let a = match args.get(0) {
                    Some(Value::Int(i)) => *i,
                    _ => return Err(self.err("random.randint() requires integers")),
                };
                let b = match args.get(1) {
                    Some(Value::Int(i)) => *i,
                    _ => return Err(self.err("random.randint() requires 2 integers")),
                };
                if a > b {
                    return Err(self.err("random.randint() a must be <= b"));
                }
                let range = (b - a + 1) as u64;
                let bits = self.xorshift64();
                Ok(Value::Int(a + (bits % range) as i64))
            }
            "uniform" => {
                let a = as_float_arg(&args, 0, "random.uniform")?;
                let b = as_float_arg(&args, 1, "random.uniform")?;
                let bits = self.xorshift64();
                let t = (bits >> 11) as f64 / (1u64 << 53) as f64;
                Ok(Value::Float(a + t * (b - a)))
            }
            "choice" => {
                let items = match args.into_iter().next() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    Some(Value::Tuple(t)) => t.as_ref().clone(),
                    _ => return Err(self.err("random.choice() requires a list or tuple")),
                };
                if items.is_empty() {
                    return Err(self.err("random.choice() called on empty sequence"));
                }
                let idx = (self.xorshift64() % items.len() as u64) as usize;
                Ok(items[idx].clone())
            }
            "shuffle" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l,
                    _ => return Err(self.err("random.shuffle() requires a list")),
                };
                let mut v = lst.borrow_mut();
                let n = v.len();
                for i in (1..n).rev() {
                    let j = (self.xorshift64() % (i as u64 + 1)) as usize;
                    v.swap(i, j);
                }
                Ok(Value::Nil)
            }
            "seed" => {
                let s = match args.into_iter().next() {
                    Some(Value::Int(i)) => i as u64,
                    Some(Value::Float(f)) => f as u64,
                    _ => return Err(self.err("random.seed() requires a number")),
                };
                self.rng_state = if s == 0 { 1 } else { s };
                Ok(Value::Nil)
            }
            _ => Err(self.err(&format!("random has no function '{}'", name))),
        }
    }

    // ── term module ───────────────────────────────────────────────────────

    fn call_term_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        use std::io::Write;
        match name {
            "raw" => {
                terminal::enable_raw_mode().map_err(|e| self.err(&format!("term.raw() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "normal" => {
                terminal::disable_raw_mode().map_err(|e| self.err(&format!("term.normal() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "clear" => {
                execute!(
                    std::io::stdout(),
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                    crossterm::cursor::MoveTo(0, 0)
                )
                .map_err(|e| self.err(&format!("term.clear() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "clear_line" => {
                execute!(
                    std::io::stdout(),
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine)
                )
                .map_err(|e| self.err(&format!("term.clear_line() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "move_cursor" => {
                let row = match args.get(0) {
                    Some(Value::Int(n)) => (*n as u16).saturating_sub(1),
                    _ => return Err(self.err("term.move_cursor(row, col) requires integers")),
                };
                let col = match args.get(1) {
                    Some(Value::Int(n)) => (*n as u16).saturating_sub(1),
                    _ => return Err(self.err("term.move_cursor(row, col) requires integers")),
                };
                execute!(std::io::stdout(), crossterm::cursor::MoveTo(col, row))
                    .map_err(|e| self.err(&format!("term.move_cursor() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "hide_cursor" => {
                execute!(std::io::stdout(), crossterm::cursor::Hide)
                    .map_err(|e| self.err(&format!("term.hide_cursor() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "show_cursor" => {
                execute!(std::io::stdout(), crossterm::cursor::Show)
                    .map_err(|e| self.err(&format!("term.show_cursor() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "write" => {
                let s = match args.into_iter().next() {
                    Some(Value::Str(s)) => s,
                    Some(v) => v.to_string(),
                    None => return Ok(Value::Nil),
                };
                print!("{}", s);
                std::io::stdout().flush().ok();
                Ok(Value::Nil)
            }
            "flush" => {
                std::io::stdout().flush().ok();
                Ok(Value::Nil)
            }
            "size" => {
                let (w, h) = terminal::size().map_err(|e| self.err(&format!("term.size() error: {}", e)))?;
                Ok(Value::Tuple(Rc::new(vec![Value::Int(w as i64), Value::Int(h as i64)])))
            }
            "get_char" => {
                // Blocking read of one keypress
                loop {
                    if let Ok(Event::Key(key)) = ct_event::read() {
                        return Ok(Value::Str(key_to_string(key)));
                    }
                }
            }
            "poll_char" => {
                // Non-blocking: wait up to `ms` milliseconds
                let ms = match args.get(0) {
                    Some(Value::Int(n)) => *n as u64,
                    Some(Value::Float(f)) => *f as u64,
                    None => 0,
                    _ => return Err(self.err("term.poll_char(ms) requires a number")),
                };
                if ct_event::poll(std::time::Duration::from_millis(ms))
                    .map_err(|e| self.err(&format!("term.poll_char() error: {}", e)))?
                {
                    if let Ok(Event::Key(key)) = ct_event::read() {
                        return Ok(Value::Str(key_to_string(key)));
                    }
                }
                Ok(Value::Nil)
            }
            _ => Err(self.err(&format!("term has no function '{}'", name))),
        }
    }

    // ── Arithmetic ────────────────────────────────────────────────────────

    fn apply_binop(&self, op: &BinOp, l: Value, r: Value) -> Result<Value, String> {
        match op {
            BinOp::Add => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(a + &b)),
                (Value::List(a), Value::List(b)) => {
                    let mut v = a.borrow().clone();
                    v.extend(b.borrow().clone());
                    Ok(Value::List(Rc::new(RefCell::new(v))))
                }
                (l, r) => Err(self.err(&format!("cannot add {} and {}", l.type_name(), r.type_name()))),
            },
            BinOp::Sub => numeric_op!(self, l, r, -, "subtract"),
            BinOp::Mul => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
                (Value::Str(s), Value::Int(n)) => Ok(Value::Str(s.repeat(n.max(0) as usize))),
                (Value::Int(n), Value::Str(s)) => Ok(Value::Str(s.repeat(n.max(0) as usize))),
                (l, r) => Err(self.err(&format!("cannot multiply {} and {}", l.type_name(), r.type_name()))),
            },
            BinOp::Div => match (l, r) {
                (_, Value::Int(0)) => Err(self.err("division by zero")),
                (_, Value::Float(f)) if f == 0.0 => Err(self.err("division by zero")),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Float(a as f64 / b as f64)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 / b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / b as f64)),
                (l, r) => Err(self.err(&format!("cannot divide {} and {}", l.type_name(), r.type_name()))),
            },
            BinOp::Mod => match (l, r) {
                (Value::Int(a), Value::Int(b)) if b != 0 => Ok(Value::Int(a % b)),
                (Value::Int(_), Value::Int(0)) => Err(self.err("modulo by zero")),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 % b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a % b as f64)),
                (l, r) => Err(self.err(&format!("cannot mod {} and {}", l.type_name(), r.type_name()))),
            },
            BinOp::Pow => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Float((a as f64).powf(b as f64))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(b))),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((a as f64).powf(b))),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a.powf(b as f64))),
                (l, r) => Err(self.err(&format!("cannot exponentiate {} and {}", l.type_name(), r.type_name()))),
            },
            BinOp::Eq => Ok(Value::Bool(values_equal(&l, &r))),
            BinOp::NotEq => Ok(Value::Bool(!values_equal(&l, &r))),
            BinOp::Lt => compare_op!(self, l, r, <),
            BinOp::LtEq => compare_op!(self, l, r, <=),
            BinOp::Gt => compare_op!(self, l, r, >),
            BinOp::GtEq => compare_op!(self, l, r, >=),
            BinOp::FloorDiv => match (l, r) {
                (_, Value::Int(0)) => Err(self.err("floor division by zero")),
                (_, Value::Float(f)) if f == 0.0 => Err(self.err("floor division by zero")),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.div_euclid(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float((a / b).floor())),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((a as f64 / b).floor())),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float((a / b as f64).floor())),
                (l, r) => Err(self.err(&format!("cannot floor-divide {} and {}", l.type_name(), r.type_name()))),
            },
            BinOp::BitAnd => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
                (l, r) => Err(self.err(&format!(
                    "bitwise & requires int, got {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::BitOr => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
                (l, r) => Err(self.err(&format!(
                    "bitwise | requires int, got {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::BitXor => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
                (l, r) => Err(self.err(&format!(
                    "bitwise ^ requires int, got {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::LShift => match (l, r) {
                (Value::Int(a), Value::Int(b)) if b >= 0 => Ok(Value::Int(a << b)),
                (Value::Int(_), Value::Int(b)) => Err(self.err(&format!("negative shift count: {}", b))),
                (l, r) => Err(self.err(&format!(
                    "shift requires int, got {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::RShift => match (l, r) {
                (Value::Int(a), Value::Int(b)) if b >= 0 => Ok(Value::Int(a >> b)),
                (Value::Int(_), Value::Int(b)) => Err(self.err(&format!("negative shift count: {}", b))),
                (l, r) => Err(self.err(&format!(
                    "shift requires int, got {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::And | BinOp::Or | BinOp::In | BinOp::NotIn => unreachable!("handled above"),
        }
    }

    // ── FFI helpers ──────────────────────────────────────────────────────────

    fn call_ffi_builtin(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "open" => {
                let path_str = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("ffi.open() requires a string path")),
                };
                let lib_path = self.resolve_ffi_lib(&path_str);
                let lib = unsafe { libloading::Library::new(&lib_path) }
                    .map_err(|e| self.err(&format!("ffi.open(\"{}\") failed: {}", lib_path, e)))?;
                Ok(Value::FfiLib(FfiLibHandle(std::sync::Arc::new(lib))))
            }
            "func" => {
                if args.len() < 3 {
                    return Err(self.err("ffi.func(lib, name, ret_type[, arg_types]) requires at least 3 args"));
                }
                let lib = match &args[0] {
                    Value::FfiLib(h) => h.clone(),
                    _ => return Err(self.err("ffi.func(): first argument must be an ffi library")),
                };
                let sym_name = match &args[1] {
                    Value::Str(s) => s.clone(),
                    _ => return Err(self.err("ffi.func(): second argument must be a string")),
                };
                let ret_type = match &args[2] {
                    Value::Str(s) => s.clone(),
                    _ => return Err(self.err("ffi.func(): third argument must be a type string")),
                };
                let arg_types: Vec<String> = if args.len() > 3 {
                    match &args[3] {
                        Value::List(lst) => lst
                            .borrow()
                            .iter()
                            .map(|v| match v {
                                Value::Str(s) => Ok(s.clone()),
                                _ => Err(self.err("ffi.func(): arg_types list must contain strings")),
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        _ => return Err(self.err("ffi.func(): fourth argument must be a list")),
                    }
                } else {
                    vec![]
                };
                // Resolve the symbol address now — fail fast if not found
                let sym_addr: usize = unsafe {
                    let sym: libloading::Symbol<*const ()> = lib
                        .0
                        .get(sym_name.as_bytes())
                        .map_err(|e| self.err(&format!("ffi.func(): symbol '{}' not found: {}", sym_name, e)))?;
                    *sym as usize
                };
                Ok(Value::FfiFunc {
                    lib,
                    sym: sym_addr,
                    name: sym_name,
                    ret_type,
                    arg_types,
                })
            }
            other => Err(self.err(&format!("ffi.{}: unknown function", other))),
        }
    }

    fn resolve_ffi_lib(&self, name: &str) -> String {
        // If it already looks like a path or has an extension, use as-is
        if name.contains('/') || name.contains('.') {
            return name.to_string();
        }
        #[cfg(target_os = "macos")]
        let candidates = [
            format!("lib{}.dylib", name),
            format!("{}.dylib", name),
            format!("/usr/lib/lib{}.dylib", name),
            format!("/usr/local/lib/lib{}.dylib", name),
            format!("/opt/homebrew/lib/lib{}.dylib", name),
        ];
        #[cfg(target_os = "linux")]
        let candidates = [
            format!("lib{}.so", name),
            format!("{}.so", name),
            format!("/usr/lib/lib{}.so", name),
            format!("/usr/local/lib/lib{}.so", name),
            format!("/lib/x86_64-linux-gnu/lib{}.so", name),
            format!("/lib/aarch64-linux-gnu/lib{}.so", name),
        ];
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let candidates = [name.to_string()];

        for c in &candidates {
            if std::path::Path::new(c).exists() {
                return c.clone();
            }
        }
        // On macOS, system libraries (libm, libc, etc.) live in the dyld shared cache
        // and are accessible by their logical name "libXYZ.dylib" even without a file on disk.
        #[cfg(target_os = "macos")]
        {
            // Try "name.dylib" first (e.g. "libm.dylib" for input "libm")
            let dylib_name = format!("{}.dylib", name);
            return dylib_name;
        }
        #[cfg(not(target_os = "macos"))]
        name.to_string()
    }

    fn invoke_ffi(
        &mut self,
        sym: usize,
        ret_type: &str,
        arg_types: &[String],
        args: Vec<Value>,
    ) -> Result<Value, String> {
        if args.len() != arg_types.len() {
            return Err(self.err(&format!(
                "FFI call: expected {} args, got {}",
                arg_types.len(),
                args.len()
            )));
        }
        // Convert each Cool value to a Slot, keeping CStrings alive for the call
        let mut cstrings: Vec<std::ffi::CString> = Vec::new();
        let mut slots: Vec<Slot> = Vec::with_capacity(args.len());
        for (i, (v, ty)) in args.iter().zip(arg_types.iter()).enumerate() {
            let slot = match ty.as_str() {
                "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" | "ptr" => {
                    let n = match v {
                        Value::Int(n) => *n,
                        Value::Bool(b) => {
                            if *b {
                                1
                            } else {
                                0
                            }
                        }
                        Value::Float(f) => *f as i64,
                        _ => {
                            return Err(self.err(&format!("FFI arg {}: cannot convert {} to {}", i, v.type_name(), ty)))
                        }
                    };
                    Slot::I(n)
                }
                "f32" | "f64" => {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        Value::Bool(b) => {
                            if *b {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        _ => {
                            return Err(self.err(&format!("FFI arg {}: cannot convert {} to {}", i, v.type_name(), ty)))
                        }
                    };
                    Slot::F(f)
                }
                "str" => {
                    let s = match v {
                        Value::Str(s) => s.clone(),
                        Value::Nil => String::new(),
                        other => other.to_string(),
                    };
                    let cstr =
                        std::ffi::CString::new(s).map_err(|_| self.err("FFI: string argument contains null byte"))?;
                    let ptr = cstr.as_ptr() as i64;
                    cstrings.push(cstr); // keep alive for the duration of the call
                    Slot::I(ptr)
                }
                other => return Err(self.err(&format!("FFI: unknown arg type '{}'", other))),
            };
            slots.push(slot);
        }
        // Perform the call; cstrings stay alive until after ffi_dispatch returns
        let result = unsafe { ffi_dispatch(sym, ret_type, arg_types, &slots) }.map_err(|e| self.err(&e))?;
        drop(cstrings);
        Ok(result)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn lookup_method(cls: &Rc<CoolClass>, name: &str) -> Option<Value> {
    if let Some(v) = cls.methods.get(name) {
        return Some(v.clone());
    }
    if let Some(parent) = &cls.parent {
        return lookup_method(parent, name);
    }
    None
}

fn is_instance_of(cls: &Rc<CoolClass>, type_name: &str) -> bool {
    if cls.name == type_name {
        return true;
    }
    if let Some(parent) = &cls.parent {
        return is_instance_of(parent, type_name);
    }
    false
}

fn exc_matches(exc_val: &Value, type_name: &str) -> bool {
    match exc_val {
        Value::Str(s) => s == type_name || type_name == "Exception" || type_name == "Error",
        Value::Instance(inst) => is_instance_of(&inst.class, type_name),
        _ => type_name == "Exception",
    }
}

pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
        (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Nil, Value::Nil) => true,
        (Value::Tuple(x), Value::Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_equal(a, b))
        }
        (Value::List(x), Value::List(y)) => {
            let x = x.borrow();
            let y = y.borrow();
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_equal(a, b))
        }
        _ => false,
    }
}

fn compare_values(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => Ok(x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Int(x), Value::Float(y)) => Ok((*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Float(x), Value::Int(y)) => Ok(x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        (a, b) => Err(format!("cannot compare {} and {}", a.type_name(), b.type_name())),
    }
}

fn to_list_index(lst: &[Value], idx: Value, line: usize) -> Result<usize, String> {
    index_into(lst.len(), &idx, line)
}

fn index_into(len: usize, idx: &Value, line: usize) -> Result<usize, String> {
    match idx {
        Value::Int(i) => {
            let i = if *i < 0 { len as i64 + i } else { *i };
            if i < 0 || i as usize >= len {
                Err(format!("line {}: index {} out of range (length {})", line, i, len))
            } else {
                Ok(i as usize)
            }
        }
        other => Err(format!("line {}: index must be int, got {}", line, other.type_name())),
    }
}
fn req_str_arg(args: &[Value], i: usize, method: &str) -> Result<String, String> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        Some(other) => Err(format!(
            "{}() argument {} must be a string, got {}",
            method,
            i + 1,
            other.type_name()
        )),
        None => Err(format!("{}() requires at least {} argument(s)", method, i + 1)),
    }
}

fn req_int_arg(args: &[Value], i: usize, method: &str) -> Result<i64, String> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(format!(
            "{}() argument {} must be an int, got {}",
            method,
            i + 1,
            other.type_name()
        )),
        None => Err(format!("{}() requires at least {} argument(s)", method, i + 1)),
    }
}

fn wrap_unsigned(n: i64, bits: u32) -> i64 {
    let mask = (1i128 << bits) - 1;
    ((n as i128) & mask) as i64
}

fn wrap_signed(n: i64, bits: u32) -> i64 {
    let modulus = 1i128 << bits;
    let sign_bit = 1i128 << (bits - 1);
    let wrapped = (n as i128) & (modulus - 1);
    if (wrapped & sign_bit) != 0 {
        (wrapped - modulus) as i64
    } else {
        wrapped as i64
    }
}

const COOL_POINTER_BITS: u32 = usize::BITS;
const COOL_POINTER_BYTES: i64 = std::mem::size_of::<usize>() as i64;

fn key_to_string(key: KeyEvent) -> String {
    // Ctrl+letter → "CTRL_X"
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = key.code {
            return format!("CTRL_{}", c.to_uppercase().next().unwrap_or(c));
        }
    }
    match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Up => "UP".to_string(),
        KeyCode::Down => "DOWN".to_string(),
        KeyCode::Left => "LEFT".to_string(),
        KeyCode::Right => "RIGHT".to_string(),
        KeyCode::Enter => "ENTER".to_string(),
        KeyCode::Backspace => "BACKSPACE".to_string(),
        KeyCode::Delete => "DELETE".to_string(),
        KeyCode::Esc => "ESC".to_string(),
        KeyCode::Tab => "TAB".to_string(),
        KeyCode::Home => "HOME".to_string(),
        KeyCode::End => "END".to_string(),
        KeyCode::PageUp => "PAGEUP".to_string(),
        KeyCode::PageDown => "PAGEDOWN".to_string(),
        _ => "UNKNOWN".to_string(),
    }
}

fn as_float_arg(args: &[Value], i: usize, method: &str) -> Result<f64, String> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n as f64),
        Some(Value::Float(f)) => Ok(*f),
        Some(other) => Err(format!(
            "{}() argument {} must be a number, got {}",
            method,
            i + 1,
            other.type_name()
        )),
        None => Err(format!("{}() requires at least {} argument(s)", method, i + 1)),
    }
}

// ── JSON parser/serializer ─────────────────────────────────────────────

fn json_dumps(v: &Value) -> String {
    match v {
        Value::Nil => "null".to_string(),
        Value::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Int(n) => n.to_string(),
        Value::Float(f) => {
            if f.is_nan() || f.is_infinite() {
                "null".to_string()
            } else {
                format!("{}", f)
            }
        }
        Value::Str(s) => {
            let mut out = String::from('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Value::List(lst) => {
            let items: Vec<String> = lst.borrow().iter().map(json_dumps).collect();
            format!("[{}]", items.join(","))
        }
        Value::Dict(map) => {
            let items: Vec<String> = map
                .borrow()
                .iter()
                .map(|(k, v)| format!("{}:{}", json_dumps(k), json_dumps(v)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
        Value::Tuple(t) => {
            let items: Vec<String> = t.iter().map(json_dumps).collect();
            format!("[{}]", items.join(","))
        }
        other => format!("\"{}\"", other),
    }
}

fn json_parse(s: &str) -> Result<Value, String> {
    let s = s.trim();
    let mut chars = s.chars().peekable();
    json_parse_value(&mut chars)
}

fn json_parse_value(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Value, String> {
    skip_ws(chars);
    match chars.peek() {
        Some('"') => json_parse_string(chars),
        Some('{') => json_parse_object(chars),
        Some('[') => json_parse_array(chars),
        Some('t') => {
            consume_literal(chars, "true")?;
            Ok(Value::Bool(true))
        }
        Some('f') => {
            consume_literal(chars, "false")?;
            Ok(Value::Bool(false))
        }
        Some('n') => {
            consume_literal(chars, "null")?;
            Ok(Value::Nil)
        }
        Some(c) if c.is_ascii_digit() || *c == '-' => json_parse_number(chars),
        Some(c) => Err(format!("unexpected JSON character '{}'", c)),
        None => Err("unexpected end of JSON".to_string()),
    }
}

fn skip_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n') | Some('\r')) {
        chars.next();
    }
}

fn consume_literal(chars: &mut std::iter::Peekable<std::str::Chars>, lit: &str) -> Result<(), String> {
    for expected in lit.chars() {
        match chars.next() {
            Some(c) if c == expected => {}
            Some(c) => return Err(format!("expected '{}' but got '{}'", expected, c)),
            None => return Err(format!("unexpected end parsing '{}'", lit)),
        }
    }
    Ok(())
}

fn json_parse_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Value, String> {
    chars.next(); // consume '"'
    let mut s = String::new();
    loop {
        match chars.next() {
            Some('"') => break,
            Some('\\') => match chars.next() {
                Some('"') => s.push('"'),
                Some('\\') => s.push('\\'),
                Some('/') => s.push('/'),
                Some('n') => s.push('\n'),
                Some('r') => s.push('\r'),
                Some('t') => s.push('\t'),
                Some('b') => s.push('\x08'),
                Some('f') => s.push('\x0C'),
                Some('u') => {
                    let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                    let cp = u32::from_str_radix(&hex, 16).map_err(|_| format!("invalid \\u escape: {}", hex))?;
                    s.push(char::from_u32(cp).unwrap_or('?'));
                }
                Some(c) => {
                    s.push('\\');
                    s.push(c);
                }
                None => return Err("unterminated string escape".to_string()),
            },
            Some(c) => s.push(c),
            None => return Err("unterminated JSON string".to_string()),
        }
    }
    Ok(Value::Str(s))
}

fn json_parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Value, String> {
    let mut s = String::new();
    let mut is_float = false;
    if chars.peek() == Some(&'-') {
        s.push(chars.next().unwrap());
    }
    while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
        s.push(chars.next().unwrap());
    }
    if chars.peek() == Some(&'.') {
        is_float = true;
        s.push(chars.next().unwrap());
        while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
            s.push(chars.next().unwrap());
        }
    }
    if matches!(chars.peek(), Some('e') | Some('E')) {
        is_float = true;
        s.push(chars.next().unwrap());
        if matches!(chars.peek(), Some('+') | Some('-')) {
            s.push(chars.next().unwrap());
        }
        while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
            s.push(chars.next().unwrap());
        }
    }
    if is_float {
        Ok(Value::Float(s.parse::<f64>().map_err(|e| e.to_string())?))
    } else {
        Ok(Value::Int(s.parse::<i64>().map_err(|e| e.to_string())?))
    }
}

fn json_parse_array(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Value, String> {
    chars.next(); // consume '['
    let mut items = Vec::new();
    skip_ws(chars);
    if chars.peek() == Some(&']') {
        chars.next();
        return Ok(Value::List(Rc::new(RefCell::new(items))));
    }
    loop {
        items.push(json_parse_value(chars)?);
        skip_ws(chars);
        match chars.next() {
            Some(']') => break,
            Some(',') => {}
            Some(c) => return Err(format!("expected ',' or ']' in JSON array, got '{}'", c)),
            None => return Err("unterminated JSON array".to_string()),
        }
    }
    Ok(Value::List(Rc::new(RefCell::new(items))))
}

fn json_parse_object(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Value, String> {
    chars.next(); // consume '{'
    let mut map = IndexedMap::new();
    skip_ws(chars);
    if chars.peek() == Some(&'}') {
        chars.next();
        return Ok(Value::Dict(Rc::new(RefCell::new(map))));
    }
    loop {
        skip_ws(chars);
        let key = json_parse_string(chars)?;
        skip_ws(chars);
        match chars.next() {
            Some(':') => {}
            Some(c) => return Err(format!("expected ':' in JSON object, got '{}'", c)),
            None => return Err("unterminated JSON object".to_string()),
        }
        let val = json_parse_value(chars)?;
        map.set(key, val);
        skip_ws(chars);
        match chars.next() {
            Some('}') => break,
            Some(',') => {}
            Some(c) => return Err(format!("expected ',' or '}}' in JSON object, got '{}'", c)),
            None => return Err("unterminated JSON object".to_string()),
        }
    }
    Ok(Value::Dict(Rc::new(RefCell::new(map))))
}

// ── FFI dispatch ──────────────────────────────────────────────────────────────

/// Argument slot for FFI dispatch: either a 64-bit integer or a 64-bit float.
#[derive(Copy, Clone)]
pub enum Slot {
    I(i64),
    F(f64),
}

/// Dispatch a C function call via raw transmuted function pointer.
///
/// * `sym`       — raw symbol address obtained from `libloading`
/// * `ret`       — declared return-type string (e.g. `"f64"`, `"i32"`, `"void"`)
/// * `arg_types` — declared argument-type strings (drives slot interpretation)
/// * `slots`     — coerced argument values (already converted from `Value`)
#[allow(clippy::too_many_arguments)]
unsafe fn ffi_dispatch(sym: usize, ret: &str, arg_types: &[String], slots: &[Slot]) -> Result<Value, String> {
    use std::ffi::CStr;
    use std::mem::transmute;
    use std::os::raw::c_char;

    // Helpers to extract typed values from slots
    macro_rules! as_i {
        ($idx:expr) => {
            match slots[$idx] {
                Slot::I(v) => v,
                Slot::F(v) => v as i64,
            }
        };
    }
    macro_rules! as_f64 {
        ($idx:expr) => {
            match slots[$idx] {
                Slot::F(v) => v,
                Slot::I(v) => v as f64,
            }
        };
    }
    macro_rules! as_f32 {
        ($idx:expr) => {
            match slots[$idx] {
                Slot::F(v) => v as f32,
                Slot::I(v) => v as f32,
            }
        };
    }

    // Convert a raw i64 return value to the correctly sign/zero-extended width
    macro_rules! int_ret {
        ($raw:expr) => {
            Ok(Value::Int(match ret {
                "i8" => $raw as i8 as i64,
                "i16" => $raw as i16 as i64,
                "i32" => $raw as i32 as i64,
                "u8" => $raw as u8 as i64,
                "u16" => $raw as u16 as i64,
                "u32" => $raw as u32 as i64,
                "isize" => wrap_signed($raw, COOL_POINTER_BITS),
                "usize" => wrap_unsigned($raw, COOL_POINTER_BITS),
                _ => $raw, // i64, u64, ptr — use as-is
            }))
        };
    }

    let n = arg_types.len();
    let is_float_ty = |t: &str| matches!(t, "f32" | "f64");

    // ── 0 args ──────────────────────────────────────────────────────────────
    if n == 0 {
        return match ret {
            "void" => {
                transmute::<_, unsafe extern "C" fn()>(sym)();
                Ok(Value::Nil)
            }
            "f32" => Ok(Value::Float(transmute::<_, unsafe extern "C" fn() -> f32>(sym)() as f64)),
            "f64" => Ok(Value::Float(transmute::<_, unsafe extern "C" fn() -> f64>(sym)())),
            "str" => {
                let p: *const c_char = transmute::<_, unsafe extern "C" fn() -> *const c_char>(sym)();
                if p.is_null() {
                    return Ok(Value::Nil);
                }
                Ok(Value::Str(CStr::from_ptr(p).to_string_lossy().into_owned()))
            }
            _ => {
                let raw = transmute::<_, unsafe extern "C" fn() -> i64>(sym)();
                int_ret!(raw)
            }
        };
    }

    // ── 1 arg ────────────────────────────────────────────────────────────────
    if n == 1 {
        let t0 = arg_types[0].as_str();
        return if t0 == "f64" {
            let a = as_f64!(0);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(f64)>(sym)(a);
                    Ok(Value::Nil)
                }
                "f32" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f64) -> f32>(sym)(a) as f64
                )),
                "f64" => Ok(Value::Float(transmute::<_, unsafe extern "C" fn(f64) -> f64>(sym)(a))),
                "str" => {
                    let p: *const c_char = transmute::<_, unsafe extern "C" fn(f64) -> *const c_char>(sym)(a);
                    if p.is_null() {
                        return Ok(Value::Nil);
                    }
                    Ok(Value::Str(CStr::from_ptr(p).to_string_lossy().into_owned()))
                }
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(f64) -> i64>(sym)(a);
                    int_ret!(raw)
                }
            }
        } else if t0 == "f32" {
            let a = as_f32!(0);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(f32)>(sym)(a);
                    Ok(Value::Nil)
                }
                "f32" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f32) -> f32>(sym)(a) as f64
                )),
                "f64" => Ok(Value::Float(transmute::<_, unsafe extern "C" fn(f32) -> f64>(sym)(a))),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(f32) -> i64>(sym)(a);
                    int_ret!(raw)
                }
            }
        } else {
            // integer-class arg (i8..i64, u8..u64, ptr, str)
            let a = as_i!(0);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(i64)>(sym)(a);
                    Ok(Value::Nil)
                }
                "f32" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(i64) -> f32>(sym)(a) as f64
                )),
                "f64" => Ok(Value::Float(transmute::<_, unsafe extern "C" fn(i64) -> f64>(sym)(a))),
                "str" => {
                    // integer arg is a char pointer
                    let p = a as *const c_char;
                    let out: *const c_char =
                        transmute::<_, unsafe extern "C" fn(*const c_char) -> *const c_char>(sym)(p);
                    if out.is_null() {
                        return Ok(Value::Nil);
                    }
                    Ok(Value::Str(CStr::from_ptr(out).to_string_lossy().into_owned()))
                }
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(i64) -> i64>(sym)(a);
                    int_ret!(raw)
                }
            }
        };
    }

    // ── 2 args ───────────────────────────────────────────────────────────────
    if n == 2 {
        let t0 = arg_types[0].as_str();
        let t1 = arg_types[1].as_str();
        return if t0 == "f64" && t1 == "f64" {
            let a = as_f64!(0);
            let b = as_f64!(1);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(f64, f64)>(sym)(a, b);
                    Ok(Value::Nil)
                }
                "f32" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f64, f64) -> f32>(sym)(a, b) as f64,
                )),
                "f64" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f64, f64) -> f64>(sym)(a, b),
                )),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(f64, f64) -> i64>(sym)(a, b);
                    int_ret!(raw)
                }
            }
        } else if t0 == "f32" && t1 == "f32" {
            let a = as_f32!(0);
            let b = as_f32!(1);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(f32, f32)>(sym)(a, b);
                    Ok(Value::Nil)
                }
                "f32" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f32, f32) -> f32>(sym)(a, b) as f64,
                )),
                "f64" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f32, f32) -> f64>(sym)(a, b),
                )),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(f32, f32) -> i64>(sym)(a, b);
                    int_ret!(raw)
                }
            }
        } else if t0 == "f64" && !is_float_ty(t1) {
            let a = as_f64!(0);
            let b = as_i!(1);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(f64, i64)>(sym)(a, b);
                    Ok(Value::Nil)
                }
                "f64" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f64, i64) -> f64>(sym)(a, b),
                )),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(f64, i64) -> i64>(sym)(a, b);
                    int_ret!(raw)
                }
            }
        } else if !is_float_ty(t0) && t1 == "f64" {
            let a = as_i!(0);
            let b = as_f64!(1);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(i64, f64)>(sym)(a, b);
                    Ok(Value::Nil)
                }
                "f64" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(i64, f64) -> f64>(sym)(a, b),
                )),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(i64, f64) -> i64>(sym)(a, b);
                    int_ret!(raw)
                }
            }
        } else {
            // both integer-class
            let a = as_i!(0);
            let b = as_i!(1);
            match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(i64, i64)>(sym)(a, b);
                    Ok(Value::Nil)
                }
                "f32" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(i64, i64) -> f32>(sym)(a, b) as f64,
                )),
                "f64" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(i64, i64) -> f64>(sym)(a, b),
                )),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(i64, i64) -> i64>(sym)(a, b);
                    int_ret!(raw)
                }
            }
        };
    }

    // ── 3 args ───────────────────────────────────────────────────────────────
    if n == 3 {
        let t0 = arg_types[0].as_str();
        let t1 = arg_types[1].as_str();
        let t2 = arg_types[2].as_str();
        if t0 == "f64" && t1 == "f64" && t2 == "f64" {
            let a = as_f64!(0);
            let b = as_f64!(1);
            let c = as_f64!(2);
            return match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(f64, f64, f64)>(sym)(a, b, c);
                    Ok(Value::Nil)
                }
                "f64" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(f64, f64, f64) -> f64>(sym)(a, b, c),
                )),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(f64, f64, f64) -> i64>(sym)(a, b, c);
                    int_ret!(raw)
                }
            };
        }
        if !is_float_ty(t0) && !is_float_ty(t1) && !is_float_ty(t2) {
            let a = as_i!(0);
            let b = as_i!(1);
            let c = as_i!(2);
            return match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(i64, i64, i64)>(sym)(a, b, c);
                    Ok(Value::Nil)
                }
                "f64" => Ok(Value::Float(
                    transmute::<_, unsafe extern "C" fn(i64, i64, i64) -> f64>(sym)(a, b, c),
                )),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(i64, i64, i64) -> i64>(sym)(a, b, c);
                    int_ret!(raw)
                }
            };
        }
    }

    // ── 4 args ───────────────────────────────────────────────────────────────
    if n == 4 {
        let t0 = arg_types[0].as_str();
        let t1 = arg_types[1].as_str();
        let t2 = arg_types[2].as_str();
        let t3 = arg_types[3].as_str();
        if t0 == "f64" && t1 == "f64" && t2 == "f64" && t3 == "f64" {
            let a = as_f64!(0);
            let b = as_f64!(1);
            let c = as_f64!(2);
            let d = as_f64!(3);
            return match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(f64, f64, f64, f64)>(sym)(a, b, c, d);
                    Ok(Value::Nil)
                }
                "f64" => Ok(Value::Float(transmute::<
                    _,
                    unsafe extern "C" fn(f64, f64, f64, f64) -> f64,
                >(sym)(a, b, c, d))),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(f64, f64, f64, f64) -> i64>(sym)(a, b, c, d);
                    int_ret!(raw)
                }
            };
        }
        if !is_float_ty(t0) && !is_float_ty(t1) && !is_float_ty(t2) && !is_float_ty(t3) {
            let a = as_i!(0);
            let b = as_i!(1);
            let c = as_i!(2);
            let d = as_i!(3);
            return match ret {
                "void" => {
                    transmute::<_, unsafe extern "C" fn(i64, i64, i64, i64)>(sym)(a, b, c, d);
                    Ok(Value::Nil)
                }
                "f64" => Ok(Value::Float(transmute::<
                    _,
                    unsafe extern "C" fn(i64, i64, i64, i64) -> f64,
                >(sym)(a, b, c, d))),
                _ => {
                    let raw = transmute::<_, unsafe extern "C" fn(i64, i64, i64, i64) -> i64>(sym)(a, b, c, d);
                    int_ret!(raw)
                }
            };
        }
    }

    Err(format!(
        "FFI: unsupported call signature ({}) -> {}",
        arg_types.join(", "),
        ret,
    ))
}
