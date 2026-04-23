/// Bytecode instruction set, chunk structure, and runtime value types for the Cool VM.
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

// ── Upvalue capture descriptor ────────────────────────────────────────────────

/// Describes how a closure captures a variable.
#[derive(Debug, Clone)]
pub enum UpvalueRef {
    /// Capture local at stack slot `n` in the immediately enclosing frame.
    Local(usize),
    /// Re-capture upvalue at index `n` from the enclosing closure.
    Upvalue(usize),
}

// ── Instructions ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Op {
    // ── Literals ──────────────────────────────────────────────────────────────
    Constant(usize), // push constants[idx]
    Nil,
    True,
    False,
    Pop,
    DupTop,
    Over,     // duplicate TOS-1 (second from top) onto TOS
    Swap,     // swap TOS and TOS-1
    RotThree, // lift TOS-2 to TOS, shift TOS and TOS-1 down one

    // ── Arithmetic ────────────────────────────────────────────────────────────
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    FloorDiv,

    // ── Comparison ────────────────────────────────────────────────────────────
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    In,
    NotIn,

    // ── Bitwise ───────────────────────────────────────────────────────────────
    BitAnd,
    BitOr,
    BitXor,
    LShift,
    RShift,

    // ── Unary ─────────────────────────────────────────────────────────────────
    Neg,
    Not,
    BitNot,

    // ── Variable access ───────────────────────────────────────────────────────
    GetLocal(usize),
    SetLocal(usize),
    GetGlobal(usize), // names[idx]
    SetGlobal(usize), // names[idx]
    GetUpvalue(usize),
    SetUpvalue(usize),

    // ── Control flow (absolute instruction indices) ───────────────────────────
    Jump(usize),
    JumpIfFalse(usize), // peek top; jump if falsy (no pop)
    JumpIfTrue(usize),  // peek top; jump if truthy (no pop)

    // ── Closures / functions ──────────────────────────────────────────────────
    /// constants[proto_idx] must be a VmValue::Proto.  upvalues are captured
    /// from the current frame according to the descriptor list.
    MakeClosure(usize, Vec<UpvalueRef>),
    /// Call top-of-stack with `argc` positional args already on stack beneath it.
    /// Stack layout: [.., func, arg0, arg1, .., argN-1]
    /// `kwarg_names` lists keyword-argument names; the matching values come after
    /// the positional args on the stack, in the same order.
    Call(usize, Vec<String>),
    Return,

    // ── Collections ───────────────────────────────────────────────────────────
    BuildList(usize),  // pop n items (top is last element), push list
    BuildDict(usize),  // pop 2n items (key, val pairs), push dict
    BuildTuple(usize), // pop n items, push tuple

    // ── Subscript / attributes ────────────────────────────────────────────────
    GetItem, // pop idx, pop obj; push obj[idx]
    SetItem, // pop val, pop idx, pop obj; obj[idx] = val
    /// pop stop, pop start, pop obj; push obj[start:stop]  (Nil = omitted end)
    GetSlice,
    GetAttr(usize), // pop obj; push obj.names[idx]
    SetAttr(usize), // pop val, pop obj; obj.names[idx] = val

    // ── Classes ───────────────────────────────────────────────────────────────
    /// Define class: name is names[idx].  If has_parent, parent class is on TOS.
    MakeClass(usize, bool),

    // ── Iteration ─────────────────────────────────────────────────────────────
    GetIter, // pop obj; push iterator
    /// Peek TOS (iterator).  If exhausted: pop iterator, jump to target.
    /// Otherwise push next value (iterator stays below it on stack).
    ForIter(usize),

    // ── Exception handling ────────────────────────────────────────────────────
    /// Push an exception handler.  If an exception is raised while this handler
    /// is active, the VM jumps to `target`, restores the stack, and pushes the
    /// exception value on TOS.
    SetupExcept(usize),
    /// Pop the current exception handler (end of try-body without exception).
    PopExcept,
    /// Pop TOS and raise it as an exception.
    Raise,
    /// Re-raise the current exception (bare `raise` inside except).
    #[allow(dead_code)]
    RaiseFrom,
    /// Peek TOS (exception); check if it matches the class whose name is names[idx].
    /// Pushes a Bool result WITHOUT consuming TOS.
    ExcMatches(usize),

    // ── Misc ──────────────────────────────────────────────────────────────────
    /// Pop iterable TOS; push `n` values from it (for tuple unpacking).
    Unpack(usize),
    /// Record source line (for error messages).
    SetLine(usize),
    /// Pop `n` string values; push their concatenation (used for f-strings).
    ConcatStr(usize),
}

// ── Chunk ─────────────────────────────────────────────────────────────────────

/// A compiled unit: instructions, constant pool, name table, debug info.
#[derive(Debug)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub constants: Vec<VmValue>,
    pub names: Vec<String>, // global/attr names
    pub lines: Vec<usize>,  // parallel to code
    pub local_count: usize,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk {
            code: Vec::new(),
            constants: Vec::new(),
            names: Vec::new(),
            lines: Vec::new(),
            local_count: 0,
        }
    }

    pub fn emit(&mut self, op: Op, line: usize) -> usize {
        let idx = self.code.len();
        self.code.push(op);
        self.lines.push(line);
        idx
    }

    pub fn add_constant(&mut self, val: VmValue) -> usize {
        // Deduplicate simple scalar constants.
        for (i, c) in self.constants.iter().enumerate() {
            match (c, &val) {
                (VmValue::Int(a), VmValue::Int(b)) if a == b => return i,
                (VmValue::Float(a), VmValue::Float(b)) if a == b => return i,
                (VmValue::Str(a), VmValue::Str(b)) if a == b => return i,
                _ => {}
            }
        }
        let idx = self.constants.len();
        self.constants.push(val);
        idx
    }

    pub fn add_name(&mut self, name: &str) -> usize {
        if let Some(i) = self.names.iter().position(|n| n == name) {
            return i;
        }
        let idx = self.names.len();
        self.names.push(name.to_string());
        idx
    }

    pub fn current_ip(&self) -> usize {
        self.code.len()
    }

    /// Emit a jump with a placeholder target; returns the instruction index.
    pub fn emit_jump<F: FnOnce(usize) -> Op>(&mut self, make_op: F, line: usize) -> usize {
        self.emit(make_op(usize::MAX), line)
    }

    /// Back-patch a jump at `idx` to point to `target`.
    pub fn patch_jump(&mut self, idx: usize, target: usize) {
        match &mut self.code[idx] {
            Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfTrue(t) | Op::ForIter(t) | Op::SetupExcept(t) => *t = target,
            other => panic!("patch_jump: not a jump at {}: {:?}", idx, other),
        }
    }
}

// ── FnProto ───────────────────────────────────────────────────────────────────

/// A compiled function prototype (not yet bound as a closure).
#[derive(Debug)]
pub struct FnProto {
    pub name: String,
    pub params: Vec<crate::ast::Param>,
    /// Pre-evaluated default values, parallel to `params`.  `None` = no default.
    pub defaults: Vec<Option<VmValue>>,
    pub chunk: Chunk,
    pub upvalue_count: usize,
    /// Total local-variable slots needed (params + locals).
    pub local_count: usize,
}

// ── Upvalue cell ──────────────────────────────────────────────────────────────

/// A shared mutable cell for a captured variable.
#[derive(Debug, Clone)]
pub struct UpvalueCell(pub Rc<RefCell<UpvalueCellInner>>);

#[derive(Debug)]
pub enum UpvalueCellInner {
    /// Variable is still live on the stack at this slot.
    Open(usize),
    /// Variable has left the stack; value is stored here.
    Closed(VmValue),
}

impl UpvalueCell {
    pub fn open(slot: usize) -> Self {
        UpvalueCell(Rc::new(RefCell::new(UpvalueCellInner::Open(slot))))
    }
    #[allow(dead_code)]
    pub fn closed(val: VmValue) -> Self {
        UpvalueCell(Rc::new(RefCell::new(UpvalueCellInner::Closed(val))))
    }
}

// ── Runtime closure ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct VmClosure {
    pub proto: Rc<FnProto>,
    pub upvalues: Vec<UpvalueCell>,
}

// ── Class & instance ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct VmClass {
    pub name: String,
    pub parent: Option<Rc<VmClass>>,
    pub methods: HashMap<String, Rc<VmClosure>>,
    pub class_vars: RefCell<HashMap<String, VmValue>>,
}

#[derive(Debug)]
pub struct VmInstance {
    pub class: Rc<VmClass>,
    pub fields: RefCell<HashMap<String, VmValue>>,
}

// ── Bound method ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct VmBoundMethod {
    pub receiver: Rc<VmInstance>,
    pub method: Rc<VmClosure>,
}

// ── File handle ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct VmFile {
    pub path: String,
    pub mode: String,
    pub content: Vec<String>,
    pub line_pos: usize,
    pub write_buf: String,
    pub closed: bool,
}

// ── Ordered dict ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct VmDict {
    pub keys: Vec<VmValue>,
    pub vals: Vec<VmValue>,
}

impl VmDict {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &VmValue) -> Option<VmValue> {
        self.keys
            .iter()
            .position(|k| vm_eq(k, key))
            .map(|i| self.vals[i].clone())
    }

    pub fn set(&mut self, key: VmValue, val: VmValue) {
        if let Some(i) = self.keys.iter().position(|k| vm_eq(k, &key)) {
            self.vals[i] = val;
        } else {
            self.keys.push(key);
            self.vals.push(val);
        }
    }

    pub fn contains(&self, key: &VmValue) -> bool {
        self.keys.iter().any(|k| vm_eq(k, key))
    }

    pub fn remove(&mut self, key: &VmValue) -> bool {
        if let Some(i) = self.keys.iter().position(|k| vm_eq(k, key)) {
            self.keys.remove(i);
            self.vals.remove(i);
            true
        } else {
            false
        }
    }
}

// ── Iterator ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum VmIter {
    List {
        items: Rc<RefCell<Vec<VmValue>>>,
        idx: usize,
    },
    Tuple {
        items: Rc<Vec<VmValue>>,
        idx: usize,
    },
    Str {
        chars: Vec<char>,
        idx: usize,
    },
    #[allow(dead_code)]
    Range {
        current: i64,
        stop: i64,
        step: i64,
    },
    DictKeys {
        dict: Rc<RefCell<VmDict>>,
        idx: usize,
    },
}

impl VmIter {
    pub fn next_val(&mut self) -> Option<VmValue> {
        match self {
            VmIter::List { items, idx } => {
                let v = items.borrow().get(*idx).cloned();
                if v.is_some() {
                    *idx += 1;
                }
                v
            }
            VmIter::Tuple { items, idx } => {
                let v = items.get(*idx).cloned();
                if v.is_some() {
                    *idx += 1;
                }
                v
            }
            VmIter::Str { chars, idx } => {
                let v = chars.get(*idx).map(|c| VmValue::Str(c.to_string()));
                if v.is_some() {
                    *idx += 1;
                }
                v
            }
            VmIter::Range { current, stop, step } => {
                let done = if *step > 0 {
                    *current >= *stop
                } else {
                    *current <= *stop
                };
                if done {
                    None
                } else {
                    let v = VmValue::Int(*current);
                    *current += *step;
                    Some(v)
                }
            }
            VmIter::DictKeys { dict, idx } => {
                let v = dict.borrow().keys.get(*idx).cloned();
                if v.is_some() {
                    *idx += 1;
                }
                v
            }
        }
    }
}

// ── Super proxy ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VmSuper {
    pub instance: Rc<VmInstance>,
    pub parent: Rc<VmClass>,
}

// ── VmValue ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum VmValue {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Nil,
    List(Rc<RefCell<Vec<VmValue>>>),
    Dict(Rc<RefCell<VmDict>>),
    Tuple(Rc<Vec<VmValue>>),
    Closure(Rc<VmClosure>),
    BoundMethod(Rc<VmBoundMethod>),
    /// A built-in method with its receiver bound (e.g. `mylist.append`).
    BoundBuiltin(Box<VmValue>, String),
    BuiltinFn(String),
    Class(Rc<VmClass>),
    Instance(Rc<VmInstance>),
    File(Rc<RefCell<VmFile>>),
    /// Used only in the constant pool; always converted to Closure at runtime.
    Proto(Rc<FnProto>),
    Iter(Rc<RefCell<VmIter>>),
    Super(Rc<VmSuper>),
}

impl VmValue {
    pub fn is_truthy(&self) -> bool {
        match self {
            VmValue::Bool(false) | VmValue::Nil => false,
            VmValue::Int(0) => false,
            VmValue::Str(s) if s.is_empty() => false,
            VmValue::List(v) => !v.borrow().is_empty(),
            VmValue::Dict(m) => !m.borrow().keys.is_empty(),
            VmValue::Tuple(t) => !t.is_empty(),
            _ => true,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            VmValue::Int(_) => "int",
            VmValue::Float(_) => "float",
            VmValue::Str(_) => "str",
            VmValue::Bool(_) => "bool",
            VmValue::Nil => "nil",
            VmValue::List(_) => "list",
            VmValue::Dict(_) => "dict",
            VmValue::Tuple(_) => "tuple",
            VmValue::Closure(_) => "function",
            VmValue::BoundMethod(_) => "method",
            VmValue::BoundBuiltin(_, _) => "builtin",
            VmValue::BuiltinFn(_) => "builtin",
            VmValue::Class(_) => "class",
            VmValue::Instance(_) => "instance",
            VmValue::File(_) => "file",
            VmValue::Proto(_) => "proto",
            VmValue::Iter(_) => "iterator",
            VmValue::Super(_) => "super",
        }
    }
}

/// Structural equality (used for `==` and dict key lookup).
pub fn vm_eq(a: &VmValue, b: &VmValue) -> bool {
    match (a, b) {
        (VmValue::Int(x), VmValue::Int(y)) => x == y,
        (VmValue::Float(x), VmValue::Float(y)) => x == y,
        (VmValue::Int(x), VmValue::Float(y)) => (*x as f64) == *y,
        (VmValue::Float(x), VmValue::Int(y)) => *x == (*y as f64),
        (VmValue::Str(x), VmValue::Str(y)) => x == y,
        (VmValue::Bool(x), VmValue::Bool(y)) => x == y,
        (VmValue::Nil, VmValue::Nil) => true,
        (VmValue::Tuple(a), VmValue::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| vm_eq(x, y))
        }
        (VmValue::List(a), VmValue::List(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| vm_eq(x, y))
        }
        _ => false,
    }
}

pub fn vm_repr(v: &VmValue) -> String {
    match v {
        VmValue::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        other => other.to_string(),
    }
}

impl fmt::Display for VmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmValue::Int(n) => write!(f, "{}", n),
            VmValue::Float(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            VmValue::Str(s) => write!(f, "{}", s),
            VmValue::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            VmValue::Nil => write!(f, "nil"),
            VmValue::List(v) => {
                let v = v.borrow();
                write!(f, "[")?;
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", vm_repr(item))?;
                }
                write!(f, "]")
            }
            VmValue::Dict(m) => {
                let m = m.borrow();
                write!(f, "{{")?;
                for (i, (k, v)) in m.keys.iter().zip(m.vals.iter()).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", vm_repr(k), vm_repr(v))?;
                }
                write!(f, "}}")
            }
            VmValue::Tuple(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", vm_repr(item))?;
                }
                if items.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            VmValue::Closure(c) => write!(f, "<function {}>", c.proto.name),
            VmValue::BoundMethod(m) => write!(f, "<method {}>", m.method.proto.name),
            VmValue::BoundBuiltin(_, name) => write!(f, "<builtin method {}>", name),
            VmValue::BuiltinFn(name) => write!(f, "<builtin {}>", name),
            VmValue::Class(c) => write!(f, "<class {}>", c.name),
            VmValue::Instance(i) => {
                // Exception instances: display the message field if present.
                if let Some(msg) = i.fields.borrow().get("message") {
                    write!(f, "{}", msg)
                } else {
                    write!(f, "<{} object>", i.class.name)
                }
            }
            VmValue::File(fh) => write!(f, "<file '{}'>", fh.borrow().path),
            VmValue::Proto(p) => write!(f, "<proto {}>", p.name),
            VmValue::Iter(_) => write!(f, "<iterator>"),
            VmValue::Super(s) => write!(f, "<super of {}>", s.parent.name),
        }
    }
}
