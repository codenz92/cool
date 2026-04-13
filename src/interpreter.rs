use crate::ast::*;
/// Tree-walk interpreter for Cool.
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

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
            "min",
            "max",
            "sum",
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
        ] {
            data.vars
                .insert(name.to_string(), Value::BuiltinFn(name.to_string()));
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
            self.root()
                .0
                .borrow_mut()
                .vars
                .insert(name.to_string(), value);
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
    /// super() proxy: holds the instance and the parent class to dispatch on
    Super {
        instance: Rc<CoolInstance>,
        parent: Rc<CoolClass>,
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
            Value::Super { parent, .. } => write!(f, "<super of {}>", parent.name),
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
            Value::Super { .. } => "super",
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

pub struct Interpreter {
    pub current_line: usize,
    pub source_dir: std::path::PathBuf,
    /// Stash for a raised exception value that must cross a Result<Value,String> boundary.
    /// Set in call_value when a user function raises; cleared when try/except catches it.
    pub pending_raise: Option<Value>,
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
    pub fn new(source_dir: std::path::PathBuf) -> Self {
        Interpreter {
            current_line: 0,
            source_dir,
            pending_raise: None,
        }
    }

    fn err(&self, msg: &str) -> String {
        format!("line {}: {}", self.current_line, msg)
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
                    return Err(self.err(&format!(
                        "unpack: expected {} values, got {}",
                        names.len(),
                        items.len()
                    )));
                }
                for (name, val) in names.iter().zip(items) {
                    env.set_local(name.clone(), val);
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

            Stmt::SetItem {
                object,
                index,
                value,
            } => {
                let obj = self.eval(object, env)?;
                let idx = self.eval(index, env)?;
                let val = self.eval(value, env)?;
                match obj {
                    Value::List(lst) => {
                        let i = to_list_index(&lst.borrow(), idx)?;
                        lst.borrow_mut()[i] = val;
                    }
                    Value::Dict(map) => {
                        map.borrow_mut().set(idx, val);
                    }
                    other => {
                        return Err(
                            self.err(&format!("cannot index-assign on {}", other.type_name()))
                        );
                    }
                }
                Ok(Signal::None)
            }

            Stmt::SetAttr {
                object,
                name,
                value,
            } => {
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
                    other => {
                        Err(self.err(&format!("cannot set attribute on {}", other.type_name())))
                    }
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

            Stmt::FnDef { name, params, body } => {
                let func = Value::Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    closure: env.clone(),
                };
                env.set_local(name.clone(), func);
                Ok(Signal::None)
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

            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                // exec_block may return Err("__raise__") when an exception crossed
                // a call boundary — recover the value from pending_raise.
                let block_result = match self.exec_block(body, env) {
                    Err(e) if e == "__raise__" => {
                        let v = self.pending_raise.take().unwrap_or(Value::Nil);
                        Ok(Signal::Raise(v))
                    }
                    other => other,
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
                let source = std::fs::read_to_string(&full_path)
                    .map_err(|e| self.err(&format!("import error: {}", e)))?;

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
                    return Ok(Signal::Raise(Value::Str(format!(
                        "AssertionError: {}",
                        msg
                    ))));
                }
                Ok(Signal::None)
            }

            Stmt::With {
                expr,
                as_name,
                body,
            } => {
                let val = self.eval(expr, env)?;
                let with_env = Env::new_child(env.clone());
                if let Some(name) = as_name {
                    with_env.set_local(name.clone(), val.clone());
                }
                let sig = self.exec_block(body, &with_env)?;
                // Auto-close file handles
                if let Value::File(fh) = &val {
                    let mut h = fh.borrow_mut();
                    if !h.closed {
                        if !h.write_buf.borrow().is_empty() {
                            let buf = h.write_buf.borrow().clone();
                            let _ = std::fs::OpenOptions::new()
                                .write(true)
                                .append(h.mode == "a")
                                .create(true)
                                .open(&h.path)
                                .and_then(|mut f| {
                                    use std::io::Write;
                                    f.write_all(buf.as_bytes())
                                });
                        }
                        h.closed = true;
                    }
                }
                Ok(sig)
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
                map.set(
                    Value::Str("pi".to_string()),
                    Value::Float(std::f64::consts::PI),
                );
                map.set(
                    Value::Str("e".to_string()),
                    Value::Float(std::f64::consts::E),
                );
                map.set(Value::Str("inf".to_string()), Value::Float(f64::INFINITY));
                map.set(Value::Str("nan".to_string()), Value::Float(f64::NAN));
                env.set_local("math".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
                // Also pull common names into scope directly
                env.set_local(
                    "sqrt".to_string(),
                    Value::BuiltinFn("math.sqrt".to_string()),
                );
                env.set_local(
                    "floor".to_string(),
                    Value::BuiltinFn("math.floor".to_string()),
                );
                env.set_local(
                    "ceil".to_string(),
                    Value::BuiltinFn("math.ceil".to_string()),
                );
                env.set_local("pi".to_string(), Value::Float(std::f64::consts::PI));
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
                os_fn!("getenv");
                os_fn!("join");
                os_fn!("mkdir");
                os_fn!("remove");
                os_fn!("rename");
                env.set_local("os".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
                env.set_local(
                    "getcwd".to_string(),
                    Value::BuiltinFn("os.getcwd".to_string()),
                );
                env.set_local(
                    "listdir".to_string(),
                    Value::BuiltinFn("os.listdir".to_string()),
                );
            }
            "sys" => {
                let argv: Vec<Value> = std::env::args().map(|a| Value::Str(a)).collect();
                let mut map = IndexedMap::new();
                map.set(
                    Value::Str("argv".to_string()),
                    Value::List(Rc::new(RefCell::new(argv))),
                );
                map.set(
                    Value::Str("exit".to_string()),
                    Value::BuiltinFn("exit".to_string()),
                );
                env.set_local("sys".to_string(), Value::Dict(Rc::new(RefCell::new(map))));
            }
            _ => {
                // Try to load as a .cool file from source_dir
                let path = self.source_dir.join(format!("{}.cool", name));
                if path.exists() {
                    let source = std::fs::read_to_string(&path)
                        .map_err(|e| self.err(&format!("import error: {}", e)))?;
                    let mut lexer = crate::lexer::Lexer::new(&source);
                    let tokens = lexer.tokenize().map_err(|e| self.err(&e))?;
                    let mut parser = crate::parser::Parser::new(tokens);
                    let program = parser.parse_program().map_err(|e| self.err(&e))?;
                    let child = Env::new_child(env.clone());
                    self.exec_block(&program, &child)?;
                    let child_vars = child.0.borrow();
                    for (k, v) in &child_vars.vars {
                        env.set_local(k.clone(), v.clone());
                    }
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
            Value::Int(n) => Err(self.err(&format!(
                "cannot iterate over int (did you mean range({}))?",
                n
            ))),
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
                        other => Err(self.err(&format!(
                            "bitwise ~ requires int, got {}",
                            other.type_name()
                        ))),
                    },
                }
            }

            Expr::BinOp { op, left, right } => {
                match op {
                    BinOp::And => {
                        let lv = self.eval(left, env)?;
                        return if !lv.is_truthy() {
                            Ok(lv)
                        } else {
                            self.eval(right, env)
                        };
                    }
                    BinOp::Or => {
                        let lv = self.eval(left, env)?;
                        return if lv.is_truthy() {
                            Ok(lv)
                        } else {
                            self.eval(right, env)
                        };
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
            Expr::Call {
                callee,
                args,
                kwargs,
            } if matches!(**callee, Expr::Attr { .. }) => {
                if let Expr::Attr { object, name } = callee.as_ref() {
                    let obj = self.eval(object, env)?;
                    let arg_vals: Result<Vec<_>, _> =
                        args.iter().map(|a| self.eval(a, env)).collect();
                    let arg_vals = arg_vals?;
                    let kwarg_vals = self.eval_kwargs(kwargs, env)?;
                    return self.call_method(obj, name, arg_vals, kwarg_vals, env);
                }
                unreachable!()
            }

            Expr::Call {
                callee,
                args,
                kwargs,
            } => {
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

            Expr::Slice {
                object,
                start,
                stop,
            } => {
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
                            return Err(self.err(&format!(
                                "slice index must be int, got {}",
                                other.type_name()
                            )));
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
                            return Err(self.err(&format!(
                                "slice index must be int, got {}",
                                other.type_name()
                            )));
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
                        Err(self.err(&format!(
                            "'{}' has no attribute '{}'",
                            inst.class.name, name
                        )))
                    }
                    Value::Str(_) | Value::List(_) | Value::Dict(_) | Value::File(_) => Ok(
                        Value::BuiltinFn(format!("<method {} on {}>", name, obj.type_name())),
                    ),
                    Value::Class(cls) => {
                        if let Some(v) = cls.methods.get(name) {
                            return Ok(v.clone());
                        }
                        Err(self.err(&format!("class '{}' has no attribute '{}'", cls.name, name)))
                    }
                    other => Err(self.err(&format!(
                        "'{}' has no attribute '{}'",
                        other.type_name(),
                        name
                    ))),
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
                let i = to_list_index(&lst.borrow(), idx)?;
                Ok(lst.borrow()[i].clone())
            }
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                let i = index_into(chars.len(), &idx)?;
                Ok(Value::Str(chars[i].to_string()))
            }
            Value::Dict(map) => map
                .borrow()
                .get(&idx)
                .ok_or_else(|| self.err(&format!("key {} not found in dict", repr(&idx)))),
            Value::Tuple(t) => {
                let i = index_into(t.len(), &idx)?;
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
        match &obj {
            Value::Str(s) => self.str_method(s.clone(), method, args),
            Value::List(_) => self.list_method(obj, method, args),
            Value::Dict(_) => self.dict_method(obj, method, args),
            Value::File(_) => self.file_method(obj, method, args),
            Value::Instance(_) => self.instance_method(obj, method, args, kwargs, env),
            Value::Super { .. } => self.super_method(obj, method, args, kwargs, env),
            other => Err(self.err(&format!(
                "'{}' has no method '{}'",
                other.type_name(),
                method
            ))),
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
                    s.split_whitespace()
                        .map(|p| Value::Str(p.to_string()))
                        .collect()
                } else {
                    let sep = req_str_arg(&args, 0, "split")?;
                    s.split(sep.as_str())
                        .map(|p| Value::Str(p.to_string()))
                        .collect()
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
            "isdigit" => Ok(Value::Bool(
                !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
            )),
            "isalpha" => Ok(Value::Bool(
                !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
            )),
            "isalnum" => Ok(Value::Bool(
                !s.is_empty() && s.chars().all(|c| c.is_alphanumeric()),
            )),
            "isupper" => Ok(Value::Bool(
                !s.is_empty() && s.chars().all(|c| c.is_uppercase()),
            )),
            "islower" => Ok(Value::Bool(
                !s.is_empty() && s.chars().all(|c| c.is_lowercase()),
            )),
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
                    let i = index_into(v.len(), idx)?;
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
                Ok(Value::Bool(
                    lst.borrow().iter().any(|x| values_equal(x, &target)),
                ))
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
                let lines: Vec<Value> = fh
                    .content
                    .iter()
                    .map(|l| Value::Str(l.clone() + "\n"))
                    .collect();
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
                    std::fs::write(&fh.path, &content)
                        .map_err(|e| self.err(&format!("file write error: {}", e)))?;
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
                params,
                body,
                closure,
                ..
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
            let expected = params
                .iter()
                .filter(|p| !p.is_vararg && !p.is_kwarg)
                .count();
            return Err(self.err(&format!("expected {} arg(s), got {}", expected, args.len())));
        }
        if !kwargs.is_empty() {
            return Err(self.err(&format!("unexpected keyword argument '{}'", kwargs[0].0)));
        }
        Ok(())
    }

    // ── Built-in functions ────────────────────────────────────────────────

    /// Evaluate kwargs, expanding `**dict` spreads into individual key-value pairs.
    fn eval_kwargs(
        &mut self,
        kwargs: &[(String, Expr)],
        env: &Env,
    ) -> Result<Vec<(String, Value)>, String> {
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
                                    return Err(self.err(&format!(
                                        "**kwargs keys must be strings, got {}",
                                        other.type_name()
                                    )));
                                }
                            }
                        }
                    }
                    other => {
                        return Err(
                            self.err(&format!("** requires a dict, got {}", other.type_name()))
                        );
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
                        Err(self.err(&format!(
                            "object of type '{}' has no len()",
                            inst.class.name
                        )))
                    }
                    Value::List(l) => Ok(Value::Int(l.borrow().len() as i64)),
                    Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                    Value::Dict(m) => Ok(Value::Int(m.borrow().keys.len() as i64)),
                    Value::Tuple(t) => Ok(Value::Int(t.len() as i64)),
                    other => {
                        Err(self.err(&format!("len() not supported for {}", other.type_name())))
                    }
                }
            }
            "range" => match args.as_slice() {
                [Value::Int(end)] => Ok(Value::List(Rc::new(RefCell::new(
                    (0..*end).map(Value::Int).collect(),
                )))),
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
                match v {
                    Value::Int(n) => Ok(Value::Int(n)),
                    Value::Float(f) => Ok(Value::Int(f as i64)),
                    Value::Str(s) => s
                        .trim()
                        .parse::<i64>()
                        .map(Value::Int)
                        .map_err(|_| self.err(&format!("cannot convert \"{}\" to int", s))),
                    Value::Bool(b) => Ok(Value::Int(if b { 1 } else { 0 })),
                    other => Err(self.err(&format!("cannot convert {} to int", other.type_name()))),
                }
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
                    other => {
                        Err(self.err(&format!("cannot convert {} to float", other.type_name())))
                    }
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
                    other => {
                        Err(self.err(&format!("abs() not supported for {}", other.type_name())))
                    }
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
                            Ok(Some(if ord == std::cmp::Ordering::Less {
                                x
                            } else {
                                cur
                            }))
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
                            Ok(Some(if ord == std::cmp::Ordering::Greater {
                                x
                            } else {
                                cur
                            }))
                        }
                    })
                    .map(|v| v.unwrap_or(Value::Nil))
            }
            "sum" => {
                let lst = match args.into_iter().next() {
                    Some(Value::List(l)) => l,
                    _ => return Err(self.err("sum() requires a list")),
                };
                lst.borrow().iter().try_fold(Value::Int(0), |acc, x| {
                    self.apply_binop(&BinOp::Add, acc, x.clone())
                })
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
                let lists: Result<Vec<Vec<Value>>, String> =
                    args.into_iter()
                        .map(|a| match a {
                            Value::List(l) => Ok(l.borrow().clone()),
                            other => Err(self
                                .err(&format!("zip() requires lists, got {}", other.type_name()))),
                        })
                        .collect();
                let lists = lists?;
                let len = lists.iter().map(|l| l.len()).min().unwrap_or(0);
                let result: Vec<Value> = (0..len)
                    .map(|i| {
                        Value::List(Rc::new(RefCell::new(
                            lists.iter().map(|l| l[i].clone()).collect(),
                        )))
                    })
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
                use std::io::Write;
                if let Some(prompt) = args.first() {
                    print!("{}", prompt);
                    std::io::stdout().flush().ok();
                }
                let mut line = String::new();
                std::io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| self.err(&format!("input() error: {}", e)))?;
                Ok(Value::Str(line.trim_end_matches('\n').to_string()))
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
                let source = std::fs::read_to_string(&path)
                    .map_err(|e| self.err(&format!("runfile: cannot read '{}': {}", path, e)))?;
                let mut lexer = crate::lexer::Lexer::new(&source);
                let tokens = lexer.tokenize().map_err(|e| self.err(&e))?;
                let mut parser = crate::parser::Parser::new(tokens);
                let program = parser.parse_program().map_err(|e| self.err(&e))?;
                let old_dir = self.source_dir.clone();
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    self.source_dir = parent.to_path_buf();
                }
                let run_env = Env::new_global();
                let result = self.exec_block(&program, &run_env);
                self.source_dir = old_dir;
                match result {
                    Err(e) if e == "__raise__" => {
                        let v = self.pending_raise.take().unwrap_or(Value::Nil);
                        eprintln!("Error in {}: {}", path, v);
                    }
                    Err(e) => eprintln!("Error in {}: {}", path, e),
                    Ok(Signal::Raise(v)) => eprintln!("Unhandled exception in {}: {}", path, v),
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
                        return Err(
                            self.err("isinstance() requires a value and a class or type name")
                        );
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
                        inst.fields.borrow().contains_key(&attr)
                            || lookup_method(&inst.class, &attr).is_some()
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
            _ => Err(self.err(&format!("unknown builtin '{}'", name))),
        }
    }

    fn call_math_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let n = match args.get(0) {
            Some(Value::Int(i)) => *i as f64,
            Some(Value::Float(f)) => *f,
            _ => return Err(self.err(&format!("math.{}() requires a number", name))),
        };
        let result = match name {
            "sqrt" => n.sqrt(),
            "floor" => n.floor(),
            "ceil" => n.ceil(),
            "abs" => n.abs(),
            "log" => n.ln(),
            "log2" => n.log2(),
            "log10" => n.log10(),
            "sin" => n.sin(),
            "cos" => n.cos(),
            "tan" => n.tan(),
            "asin" => n.asin(),
            "acos" => n.acos(),
            "atan" => n.atan(),
            "atan2" => {
                let m = match args.get(1) {
                    Some(Value::Int(i)) => *i as f64,
                    Some(Value::Float(f)) => *f,
                    _ => return Err(self.err("math.atan2() requires 2 numbers")),
                };
                n.atan2(m)
            }
            "pow" => {
                let exp = match args.get(1) {
                    Some(Value::Int(i)) => *i as f64,
                    Some(Value::Float(f)) => *f,
                    _ => return Err(self.err("math.pow() requires 2 numbers")),
                };
                n.powf(exp)
            }
            _ => return Err(self.err(&format!("math has no function '{}'", name))),
        };
        if result.fract() == 0.0 && matches!(name, "floor" | "ceil") {
            Ok(Value::Int(result as i64))
        } else {
            Ok(Value::Float(result))
        }
    }

    fn call_os_fn(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "getcwd" => {
                let path = std::env::current_dir()
                    .map_err(|e| self.err(&format!("os.getcwd() error: {}", e)))?;
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
            "join" => {
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
                std::fs::create_dir_all(&path)
                    .map_err(|e| self.err(&format!("os.mkdir() error: {}", e)))?;
                Ok(Value::Nil)
            }
            "remove" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err(self.err("os.remove() requires a path string")),
                };
                std::fs::remove_file(&path)
                    .map_err(|e| self.err(&format!("os.remove() error: {}", e)))?;
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
                std::fs::rename(&from, &to)
                    .map_err(|e| self.err(&format!("os.rename() error: {}", e)))?;
                Ok(Value::Nil)
            }
            _ => Err(self.err(&format!("os has no function '{}'", name))),
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
                (l, r) => Err(self.err(&format!(
                    "cannot add {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::Sub => numeric_op!(self, l, r, -, "subtract"),
            BinOp::Mul => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
                (Value::Str(s), Value::Int(n)) => Ok(Value::Str(s.repeat(n.max(0) as usize))),
                (Value::Int(n), Value::Str(s)) => Ok(Value::Str(s.repeat(n.max(0) as usize))),
                (l, r) => Err(self.err(&format!(
                    "cannot multiply {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::Div => match (l, r) {
                (_, Value::Int(0)) => Err(self.err("division by zero")),
                (_, Value::Float(f)) if f == 0.0 => Err(self.err("division by zero")),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Float(a as f64 / b as f64)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 / b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / b as f64)),
                (l, r) => Err(self.err(&format!(
                    "cannot divide {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::Mod => match (l, r) {
                (Value::Int(a), Value::Int(b)) if b != 0 => Ok(Value::Int(a % b)),
                (Value::Int(_), Value::Int(0)) => Err(self.err("modulo by zero")),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 % b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a % b as f64)),
                (l, r) => Err(self.err(&format!(
                    "cannot mod {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::Pow => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Float((a as f64).powf(b as f64))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(b))),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((a as f64).powf(b))),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a.powf(b as f64))),
                (l, r) => Err(self.err(&format!(
                    "cannot exponentiate {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
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
                (l, r) => Err(self.err(&format!(
                    "cannot floor-divide {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
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
                (Value::Int(_), Value::Int(b)) => {
                    Err(self.err(&format!("negative shift count: {}", b)))
                }
                (l, r) => Err(self.err(&format!(
                    "shift requires int, got {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::RShift => match (l, r) {
                (Value::Int(a), Value::Int(b)) if b >= 0 => Ok(Value::Int(a >> b)),
                (Value::Int(_), Value::Int(b)) => {
                    Err(self.err(&format!("negative shift count: {}", b)))
                }
                (l, r) => Err(self.err(&format!(
                    "shift requires int, got {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinOp::And | BinOp::Or | BinOp::In | BinOp::NotIn => unreachable!("handled above"),
        }
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
        _ => false,
    }
}

fn compare_values(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => {
            Ok(x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
        }
        (Value::Int(x), Value::Float(y)) => Ok((*x as f64)
            .partial_cmp(y)
            .unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Float(x), Value::Int(y)) => Ok(x
            .partial_cmp(&(*y as f64))
            .unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        (a, b) => Err(format!(
            "cannot compare {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

fn to_list_index(lst: &[Value], idx: Value) -> Result<usize, String> {
    index_into(lst.len(), &idx)
}

fn index_into(len: usize, idx: &Value) -> Result<usize, String> {
    match idx {
        Value::Int(i) => {
            let i = if *i < 0 { len as i64 + i } else { *i };
            if i < 0 || i as usize >= len {
                Err(format!("index {} out of range (length {})", i, len))
            } else {
                Ok(i as usize)
            }
        }
        other => Err(format!("index must be int, got {}", other.type_name())),
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
        None => Err(format!(
            "{}() requires at least {} argument(s)",
            method,
            i + 1
        )),
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
        None => Err(format!(
            "{}() requires at least {} argument(s)",
            method,
            i + 1
        )),
    }
}
