/// Stack-based bytecode VM for Cool.
use crate::argparse_runtime::{self, ArgData};
use crate::core_runtime;
use crate::csv_runtime;
use crate::datetime_runtime::{self, DateTimeParts};
use crate::hashlib_runtime;
use crate::http_runtime;
use crate::logging_runtime::{self, LogData, LogLevel};
use crate::module_exports;
use crate::project::ModuleResolver;
use crate::sqlite_runtime::{self, SqlData};
use crate::toml_runtime::{self, TomlData};
use crate::yaml_runtime::{self, YamlData};
use crossterm::event::{self as ct_event, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use crate::opcode::*;
use crate::subprocess_runtime::{run_shell_command, SubprocessResult};

// ── Call frame ────────────────────────────────────────────────────────────────

struct CallFrame {
    closure: Rc<VmClosure>,
    ip: usize,
    /// Index into `VM::stack` where this frame's locals start.
    base: usize,
}

// ── Exception handler entry ───────────────────────────────────────────────────

struct ExcHandler {
    handler_ip: usize,
    stack_depth: usize,
    frame_depth: usize,
}

// ── VM ────────────────────────────────────────────────────────────────────────

pub struct VM {
    stack: Vec<VmValue>,
    frames: Vec<CallFrame>,
    globals: HashMap<String, VmValue>,
    exc_handlers: Vec<ExcHandler>,
    /// Currently active exception (for bare `raise`).
    current_exc: Option<VmValue>,
    source_dir: std::path::PathBuf,
    module_resolver: ModuleResolver,
    current_line: usize,
    /// xorshift64 RNG state.
    rng: u64,
    /// All currently-open upvalue cells (regardless of which closure owns them).
    /// Lets us close upvalues for any slot when it leaves the stack.
    open_upvalues: Vec<UpvalueCell>,
    importing_modules: Vec<std::path::PathBuf>,
    logging_state: logging_runtime::LoggingState,
}

impl VM {
    pub fn new(source_dir: std::path::PathBuf, module_resolver: ModuleResolver) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(12345678901234567);
        let mut vm = VM {
            stack: Vec::with_capacity(256),
            frames: Vec::new(),
            globals: HashMap::new(),
            exc_handlers: Vec::new(),
            current_exc: None,
            source_dir,
            module_resolver,
            current_line: 0,
            rng: if seed == 0 { 1 } else { seed },
            open_upvalues: Vec::new(),
            importing_modules: Vec::new(),
            logging_state: logging_runtime::LoggingState::default(),
        };
        vm.register_builtins();
        vm
    }

    fn register_builtins(&mut self) {
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
            "repr",
            "exit",
            "open",
            "isinstance",
            "hasattr",
            "getattr",
            "list",
            "tuple",
            "dict",
            "set",
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
            "set_completions",
            "eval",
            "append",
            "pop",
            "keys",
            "values",
            "items",
            "runfile",
            "super",
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
            "__import_file__",
            "__import_module__",
            "__exc_matches__",
        ] {
            self.globals
                .insert(name.to_string(), VmValue::BuiltinFn(name.to_string()));
        }
        // Built-in exception classes (used in `except ExcType as e:` clauses).
        for exc_name in &[
            "Exception",
            "ValueError",
            "TypeError",
            "RuntimeError",
            "IndexError",
            "KeyError",
            "AttributeError",
            "NameError",
            "StopIteration",
            "IOError",
            "ZeroDivisionError",
            "NotImplementedError",
            "OverflowError",
        ] {
            let cls = Rc::new(VmClass {
                name: exc_name.to_string(),
                parent: None,
                methods: std::collections::HashMap::new(),
                class_vars: RefCell::new(HashMap::new()),
            });
            self.globals.insert(exc_name.to_string(), VmValue::Class(cls));
        }
    }

    // ── Error helpers ─────────────────────────────────────────────────────────

    fn err(&self, msg: &str) -> String {
        format!("RuntimeError (line {}): {}", self.current_line, msg)
    }

    fn type_err(&self, expected: &str, got: &VmValue) -> String {
        self.err(&format!("expected {}, got {}", expected, got.type_name()))
    }

    // ── RNG helpers (xorshift64) ──────────────────────────────────────────────

    fn rng_next_u64(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn rng_next_f64(&mut self) -> f64 {
        (self.rng_next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    // ── Stack helpers ─────────────────────────────────────────────────────────

    fn push(&mut self, v: VmValue) {
        self.stack.push(v);
    }

    fn pop(&mut self) -> VmValue {
        self.stack.pop().expect("stack underflow")
    }

    fn peek(&self) -> &VmValue {
        self.stack.last().expect("stack underflow")
    }

    #[allow(dead_code)]
    fn peek_mut(&mut self) -> &mut VmValue {
        self.stack.last_mut().expect("stack underflow")
    }

    // ── Upvalue helpers ───────────────────────────────────────────────────────

    fn read_upvalue(&self, cell: &UpvalueCell) -> VmValue {
        match &*cell.0.borrow() {
            UpvalueCellInner::Open(slot) => self.stack[*slot].clone(),
            UpvalueCellInner::Closed(v) => v.clone(),
        }
    }

    fn write_upvalue(&mut self, cell: &UpvalueCell, val: VmValue) {
        match &mut *cell.0.borrow_mut() {
            UpvalueCellInner::Open(slot) => {
                let s = *slot;
                self.stack[s] = val;
            }
            UpvalueCellInner::Closed(v) => {
                *v = val;
            }
        }
    }

    // ── Main execution loop ───────────────────────────────────────────────────

    pub fn run(&mut self, chunk: &crate::opcode::Chunk) -> Result<(), String> {
        // Wrap the top-level chunk in a synthetic closure.
        let proto = Rc::new(FnProto {
            name: "<script>".to_string(),
            params: vec![],
            defaults: vec![],
            chunk: {
                // We can't move chunk; clone its contents into a Chunk.
                let mut c = Chunk::new();
                c.code = chunk.code.clone();
                c.constants = chunk.constants.clone();
                c.names = chunk.names.clone();
                c.lines = chunk.lines.clone();
                c.local_count = chunk.local_count;
                c
            },
            upvalue_count: 0,
            local_count: chunk.local_count,
        });
        let closure = Rc::new(VmClosure {
            proto,
            upvalues: vec![],
        });
        // base = current stack height so that Op::Return truncates back to here,
        // not to 0 (which would destroy callers' locals when run() is called mid-execution).
        let base = self.stack.len();
        self.stack.resize(base + chunk.local_count, VmValue::Nil);
        self.frames.push(CallFrame { closure, ip: 0, base });

        match self.execute() {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn execute(&mut self) -> Result<VmValue, String> {
        let entry_frames = self.frames.len();
        loop {
            let (op, base) = {
                let frame = self.frames.last().unwrap();
                let op = frame.closure.proto.chunk.code[frame.ip].clone();
                (op, frame.base)
            };
            self.frames.last_mut().unwrap().ip += 1;

            // Run one instruction. On Err, route through exception handlers if any are installed.
            match self.dispatch_op(op, base, entry_frames) {
                Ok(None) => {}               // normal: continue loop
                Ok(Some(v)) => return Ok(v), // Return instruction
                Err(e) => {
                    let exc = if e.starts_with("Unhandled exception (line ") {
                        self.current_exc.clone().unwrap_or_else(|| VmValue::Str(e.clone()))
                    } else {
                        VmValue::Str(e.clone())
                    };
                    self.current_exc = Some(exc.clone());
                    if !self.exc_handlers.is_empty() {
                        return self.handle_exception(exc);
                    }
                    return Err(e);
                }
            }
        }
    }

    fn dispatch_op(&mut self, op: Op, base: usize, entry_frames: usize) -> Result<Option<VmValue>, String> {
        // Save frame count before dispatching. After any user-code call (call_closure,
        // call_value), check this: if frames dropped, it means exception handling ran
        // the handler (and possibly the rest of the script) in a recursive execute()
        // context, consuming our frames. We must signal an early exit in that case.
        let frames_before = self.frames.len();
        macro_rules! check_frames {
            ($v:expr) => {{
                if self.frames.len() < frames_before {
                    return Ok(Some($v));
                }
            }};
        }
        match op {
            Op::SetLine(n) => {
                self.current_line = n;
            }

            Op::Constant(idx) => {
                let v = self.frames.last().unwrap().closure.proto.chunk.constants[idx].clone();
                self.push(v);
            }
            Op::Nil => self.push(VmValue::Nil),
            Op::True => self.push(VmValue::Bool(true)),
            Op::False => self.push(VmValue::Bool(false)),
            Op::Pop => {
                self.pop();
            }
            Op::DupTop => {
                let v = self.peek().clone();
                self.push(v);
            }
            Op::Over => {
                let len = self.stack.len();
                let v = self.stack[len - 2].clone();
                self.push(v);
            }
            Op::Swap => {
                let len = self.stack.len();
                self.stack.swap(len - 1, len - 2);
            }
            Op::RotThree => {
                // Lift TOS-2 to TOS; TOS and TOS-1 shift down one.
                // [.., a, b, c] (TOS=c) → [.., b, c, a]
                let len = self.stack.len();
                let a = self.stack.remove(len - 3);
                self.stack.push(a);
            }

            // ── Arithmetic ─────────────────────────────────────────────
            Op::Add => {
                let r = self.pop();
                let l = self.pop();
                // Check for __add__ before numeric add.
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__add__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(self.add(l, r)?);
            }
            Op::Sub => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__sub__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(self.arith(l, r, "-")?);
            }
            Op::Mul => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__mul__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(self.mul(l, r)?);
            }
            Op::Div => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.div(l, r)?);
            }
            Op::Mod => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.modulo(l, r)?);
            }
            Op::Pow => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.pow(l, r)?);
            }
            Op::FloorDiv => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.floor_div(l, r)?);
            }

            // ── Comparison ─────────────────────────────────────────────
            Op::Eq => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__eq__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(VmValue::Bool(vm_eq(&l, &r)));
            }
            Op::NotEq => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__eq__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        let b = result.is_truthy();
                        self.push(VmValue::Bool(!b));
                        return Ok(None);
                    }
                }
                self.push(VmValue::Bool(!vm_eq(&l, &r)));
            }
            Op::Lt => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__lt__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(self.cmp_op(l, r, "<")?);
            }
            Op::LtEq => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__le__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(self.cmp_op(l, r, "<=")?);
            }
            Op::Gt => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__gt__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(self.cmp_op(l, r, ">")?);
            }
            Op::GtEq => {
                let r = self.pop();
                let l = self.pop();
                if let VmValue::Instance(ref inst) = l {
                    if let Some(m) = self.find_method(&inst.class, "__ge__") {
                        let result = self.call_closure(m, &[l, r], &[])?;
                        check_frames!(result.clone());
                        self.push(result);
                        return Ok(None);
                    }
                }
                self.push(self.cmp_op(l, r, ">=")?);
            }
            Op::In => {
                let r = self.pop();
                let l = self.pop();
                self.push(VmValue::Bool(self.contains(&r, &l)?));
            }
            Op::NotIn => {
                let r = self.pop();
                let l = self.pop();
                self.push(VmValue::Bool(!self.contains(&r, &l)?));
            }

            // ── Bitwise ────────────────────────────────────────────────
            Op::BitAnd => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.bitop(l, r, "&")?);
            }
            Op::BitOr => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.bitop(l, r, "|")?);
            }
            Op::BitXor => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.bitop(l, r, "^")?);
            }
            Op::LShift => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.bitop(l, r, "<<")?);
            }
            Op::RShift => {
                let r = self.pop();
                let l = self.pop();
                self.push(self.bitop(l, r, ">>")?);
            }

            // ── Unary ──────────────────────────────────────────────────
            Op::Neg => {
                let v = self.pop();
                self.push(match v {
                    VmValue::Int(n) => VmValue::Int(-n),
                    VmValue::Float(f) => VmValue::Float(-f),
                    other => return Err(self.type_err("number", &other)),
                });
            }
            Op::Not => {
                let v = self.pop();
                self.push(VmValue::Bool(!v.is_truthy()));
            }
            Op::BitNot => {
                let v = self.pop();
                match v {
                    VmValue::Int(n) => self.push(VmValue::Int(!n)),
                    other => return Err(self.type_err("int", &other)),
                }
            }

            // ── Variables ──────────────────────────────────────────────
            Op::GetLocal(slot) => {
                let v = self.stack[base + slot].clone();
                self.push(v);
            }
            Op::SetLocal(slot) => {
                let v = self.pop();
                let idx = base + slot;
                if idx >= self.stack.len() {
                    self.stack.resize(idx + 1, VmValue::Nil);
                }
                self.stack[idx] = v;
            }
            Op::GetGlobal(name_idx) => {
                let name = self.frames.last().unwrap().closure.proto.chunk.names[name_idx].clone();
                let v = self
                    .globals
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| self.err(&format!("undefined variable '{}'", name)))?;
                self.push(v);
            }
            Op::SetGlobal(name_idx) => {
                let name = self.frames.last().unwrap().closure.proto.chunk.names[name_idx].clone();
                let v = self.pop();
                self.globals.insert(name, v);
            }
            Op::GetUpvalue(idx) => {
                let cell = self.frames.last().unwrap().closure.upvalues[idx].clone();
                let v = self.read_upvalue(&cell);
                self.push(v);
            }
            Op::SetUpvalue(idx) => {
                let cell = self.frames.last().unwrap().closure.upvalues[idx].clone();
                let v = self.pop();
                self.write_upvalue(&cell, v);
            }

            // ── Control flow ───────────────────────────────────────────
            Op::Jump(target) => {
                self.frames.last_mut().unwrap().ip = target;
            }
            Op::JumpIfFalse(target) => {
                if !self.peek().is_truthy() {
                    self.frames.last_mut().unwrap().ip = target;
                }
            }
            Op::JumpIfTrue(target) => {
                if self.peek().is_truthy() {
                    self.frames.last_mut().unwrap().ip = target;
                }
            }

            // ── Closures / calls ───────────────────────────────────────
            Op::MakeClosure(proto_idx, refs) => {
                let proto = match &self.frames.last().unwrap().closure.proto.chunk.constants[proto_idx] {
                    VmValue::Proto(p) => p.clone(),
                    other => return Err(self.err(&format!("MakeClosure: not a proto: {}", other.type_name()))),
                };
                let mut upvalues: Vec<UpvalueCell> = Vec::new();
                for r in &refs {
                    let cell = match r {
                        UpvalueRef::Local(slot) => {
                            let abs = base + slot;
                            // Reuse an existing open cell for this slot if one exists.
                            if let Some(existing) = self
                                .open_upvalues
                                .iter()
                                .find(|c| matches!(*c.0.borrow(), UpvalueCellInner::Open(s) if s == abs))
                            {
                                existing.clone()
                            } else {
                                let cell = UpvalueCell::open(abs);
                                self.open_upvalues.push(cell.clone());
                                cell
                            }
                        }
                        UpvalueRef::Upvalue(idx) => {
                            // Inherit from enclosing closure — already tracked.
                            self.frames.last().unwrap().closure.upvalues[*idx].clone()
                        }
                    };
                    upvalues.push(cell);
                }
                self.push(VmValue::Closure(Rc::new(VmClosure { proto, upvalues })));
            }

            Op::Call(argc, kwarg_names) => {
                let result = self.call_value(argc, &kwarg_names)?;
                check_frames!(result.clone());
                self.push(result);
            }

            Op::Return => {
                let val = self.pop();
                let frame_base = self.frames.last().unwrap().base;
                let returning_depth = self.frames.len();
                self.close_upvalues_above(frame_base);
                self.stack.truncate(frame_base);
                self.frames.pop();
                // Clean up any exc_handlers that were installed by the returning frame.
                // Without this, a `return` inside a `try` block would leave stale handlers.
                self.exc_handlers.retain(|h| h.frame_depth < returning_depth);
                if self.frames.len() < entry_frames {
                    return Ok(Some(val));
                }
                self.push(val);
            }

            // ── Collections ────────────────────────────────────────────
            Op::BuildList(n) => {
                let start = self.stack.len() - n;
                let items: Vec<VmValue> = self.stack.drain(start..).collect();
                self.push(VmValue::List(Rc::new(RefCell::new(items))));
            }
            Op::BuildDict(n) => {
                let start = self.stack.len() - n * 2;
                let pairs: Vec<VmValue> = self.stack.drain(start..).collect();
                let mut d = VmDict::new();
                for chunk in pairs.chunks(2) {
                    d.set(chunk[0].clone(), chunk[1].clone());
                }
                self.push(VmValue::Dict(Rc::new(RefCell::new(d))));
            }
            Op::BuildTuple(n) => {
                let start = self.stack.len() - n;
                let items: Vec<VmValue> = self.stack.drain(start..).collect();
                self.push(VmValue::Tuple(Rc::new(items)));
            }

            // ── Subscript / attrs ──────────────────────────────────────
            Op::GetItem => {
                let idx = self.pop();
                let obj = self.pop();
                self.push(self.get_item(obj, idx)?);
            }
            Op::SetItem => {
                let val = self.pop();
                let idx = self.pop();
                let obj = self.pop();
                self.set_item(obj, idx, val)?;
            }
            Op::GetSlice => {
                let stop = self.pop();
                let start = self.pop();
                let obj = self.pop();
                self.push(self.get_slice(obj, start, stop)?);
            }
            Op::GetAttr(name_idx) => {
                let name = self.frames.last().unwrap().closure.proto.chunk.names[name_idx].clone();
                let obj = self.pop();
                self.push(self.get_attr(obj, &name)?);
            }
            Op::SetAttr(name_idx) => {
                let name = self.frames.last().unwrap().closure.proto.chunk.names[name_idx].clone();
                let val = self.pop();
                let obj = self.pop();
                self.set_attr(obj, &name, val)?;
            }

            // ── Classes ────────────────────────────────────────────────
            Op::MakeClass(name_idx, has_parent) => {
                let name = self.frames.last().unwrap().closure.proto.chunk.names[name_idx].clone();
                let parent = if has_parent {
                    match self.pop() {
                        VmValue::Class(c) => Some(c),
                        other => {
                            return Err(self.err(&format!("class parent must be a class, got {}", other.type_name())))
                        }
                    }
                } else {
                    None
                };
                self.push(VmValue::Class(Rc::new(VmClass {
                    name,
                    parent,
                    methods: HashMap::new(),
                    class_vars: RefCell::new(HashMap::new()),
                })));
            }

            // ── Iteration ──────────────────────────────────────────────
            Op::GetIter => {
                let obj = self.pop();
                let iter = self.make_iter(obj)?;
                self.push(VmValue::Iter(Rc::new(RefCell::new(iter))));
            }
            Op::ForIter(target) => {
                let next = match self.peek() {
                    VmValue::Iter(it) => it.borrow_mut().next_val(),
                    other => return Err(self.err(&format!("ForIter: not an iterator: {}", other.type_name()))),
                };
                match next {
                    Some(v) => self.push(v),
                    None => {
                        self.pop(); // pop the exhausted iterator
                        self.frames.last_mut().unwrap().ip = target;
                    }
                }
            }

            // ── Exception handling ─────────────────────────────────────
            Op::SetupExcept(target) => {
                self.exc_handlers.push(ExcHandler {
                    handler_ip: target,
                    stack_depth: self.stack.len(),
                    frame_depth: self.frames.len(),
                });
            }
            Op::PopExcept => {
                self.exc_handlers.pop();
            }
            Op::Raise => {
                let val = self.pop();
                let exc = match &val {
                    VmValue::Class(_) => self.call_value_direct(val.clone(), &[], &[])?,
                    _ => val,
                };
                self.current_exc = Some(exc.clone());
                return self.handle_exception(exc).map(Some);
            }
            Op::RaiseFrom => {
                let exc = self.current_exc.clone().unwrap_or(VmValue::Nil);
                return self.handle_exception(exc).map(Some);
            }
            Op::ExcMatches(name_idx) => {
                // Peek TOS (exception); check if it matches the named class.
                let exc = self.peek().clone();
                let chunk = &self.frames.last().unwrap().closure.proto.chunk;
                let class_name = chunk.names.get(name_idx).cloned().unwrap_or_default();
                let matches = match &exc {
                    VmValue::Instance(inst) => {
                        // Look up the class in globals by name.
                        if let Some(VmValue::Class(cls)) = self.globals.get(&class_name) {
                            let cls = cls.clone();
                            self.is_instance_of(&inst.class, &cls)
                        } else {
                            inst.class.name == class_name
                        }
                    }
                    VmValue::Str(_) | VmValue::Nil => {
                        // String exceptions match Exception, ValueError, TypeError etc.
                        class_name == "Exception"
                            || class_name == "ValueError"
                            || class_name == "TypeError"
                            || class_name == "RuntimeError"
                            || class_name == "ZeroDivisionError"
                            || class_name == "IndexError"
                            || class_name == "KeyError"
                            || class_name == "NameError"
                            || class_name == "AttributeError"
                            || class_name == "IOError"
                            || class_name == "StopIteration"
                    }
                    _ => class_name == "Exception",
                };
                self.push(VmValue::Bool(matches));
            }

            // ── Misc ───────────────────────────────────────────────────
            Op::Unpack(n) => {
                let val = self.pop();
                let items = self.to_iter_vec(val)?;
                if items.len() != n {
                    return Err(self.err(&format!(
                        "not enough values to unpack (expected {}, got {})",
                        n,
                        items.len()
                    )));
                }
                for item in items {
                    self.push(item);
                }
            }
            Op::ConcatStr(n) => {
                let start = self.stack.len() - n;
                let parts: Vec<VmValue> = self.stack.drain(start..).collect();
                let mut result = String::new();
                for p in parts {
                    result.push_str(&p.to_string());
                }
                self.push(VmValue::Str(result));
            }
        }
        Ok(None)
    }

    // ── Exception dispatch ────────────────────────────────────────────────────

    fn handle_exception(&mut self, exc: VmValue) -> Result<VmValue, String> {
        // Search for a handler, unwinding frames as needed.
        while let Some(handler) = self.exc_handlers.pop() {
            // Unwind frames to the one that installed the handler.
            while self.frames.len() > handler.frame_depth {
                let fb = self.frames.last().unwrap().base;
                self.close_upvalues_above(fb);
                self.stack.truncate(fb);
                self.frames.pop();
            }
            // Restore stack depth.
            self.stack.truncate(handler.stack_depth);
            // Push the exception.
            self.push(exc.clone());
            // Jump to handler.
            self.frames.last_mut().unwrap().ip = handler.handler_ip;
            // Continue executing.
            return self.execute();
        }
        // No handler found.
        Err(format!("Unhandled exception (line {}): {}", self.current_line, exc))
    }

    // ── Upvalue closing ───────────────────────────────────────────────────────

    /// Close all open upvalue cells that point to stack slots >= `min_slot`.
    /// Scans the global open_upvalues list so closures stored as values are covered too.
    fn close_upvalues_above(&mut self, min_slot: usize) {
        for cell in &self.open_upvalues {
            let mut inner = cell.0.borrow_mut();
            if let UpvalueCellInner::Open(slot) = *inner {
                if slot >= min_slot {
                    let val = self.stack.get(slot).cloned().unwrap_or(VmValue::Nil);
                    *inner = UpvalueCellInner::Closed(val);
                }
            }
        }
        // Drop cells that are now closed (they hold no Open reference anymore).
        self.open_upvalues
            .retain(|c| matches!(*c.0.borrow(), UpvalueCellInner::Open(_)));
    }

    // ── Function calls ────────────────────────────────────────────────────────

    /// Pop function + args from stack (layout: [func, arg0..argN-1, kwval0..]) and call.
    fn call_value(&mut self, argc: usize, kwarg_names: &[String]) -> Result<VmValue, String> {
        let total = argc + kwarg_names.len();
        // Stack: [.., func, arg0, .., argN-1, kw0, .., kwM-1]
        let func_pos = self.stack.len() - total - 1;
        let func = self.stack[func_pos].clone();

        // Collect kwargs.
        let kwarg_vals: Vec<VmValue> = self.stack.drain(self.stack.len() - kwarg_names.len()..).collect();
        let kwargs: Vec<(String, VmValue)> = kwarg_names.iter().cloned().zip(kwarg_vals).collect();

        // Collect positional args.
        let args: Vec<VmValue> = self.stack.drain(func_pos + 1..).collect();

        // Remove the function.
        self.stack.pop();

        self.call_value_direct(func, &args, &kwargs)
    }

    fn call_value_direct(
        &mut self,
        func: VmValue,
        args: &[VmValue],
        kwargs: &[(String, VmValue)],
    ) -> Result<VmValue, String> {
        match func {
            VmValue::Closure(closure) => self.call_closure(closure, args, kwargs),
            VmValue::BoundMethod(bm) => {
                // Prepend `self` to args.
                let mut full_args = vec![VmValue::Instance(bm.receiver.clone())];
                full_args.extend_from_slice(args);
                self.call_closure(bm.method.clone(), &full_args, kwargs)
            }
            VmValue::BoundBuiltin(receiver, name) => {
                // Prepend receiver as args[0], then dispatch as normal builtin.
                let mut full_args = vec![*receiver];
                full_args.extend_from_slice(args);
                self.call_builtin(&name, &full_args, kwargs)
            }
            VmValue::Class(cls) => {
                // Built-in exception classes: just return the message string.
                let exc_names = [
                    "Exception",
                    "ValueError",
                    "TypeError",
                    "RuntimeError",
                    "IndexError",
                    "KeyError",
                    "AttributeError",
                    "NameError",
                    "StopIteration",
                    "IOError",
                    "ZeroDivisionError",
                    "NotImplementedError",
                    "OverflowError",
                ];
                if exc_names.contains(&cls.name.as_str()) && cls.methods.is_empty() {
                    // Return the message as a string, or an instance with a message field.
                    let msg = args.first().map(|v| v.to_string()).unwrap_or_default();
                    // Return instance with 'message' and 'args' fields for compat.
                    let instance = Rc::new(VmInstance {
                        class: cls.clone(),
                        fields: RefCell::new({
                            let mut m = HashMap::new();
                            m.insert("message".to_string(), VmValue::Str(msg.clone()));
                            m.insert("args".to_string(), VmValue::Tuple(Rc::new(args.to_vec())));
                            m
                        }),
                    });
                    return Ok(VmValue::Instance(instance));
                }
                // Instantiate.
                let instance = Rc::new(VmInstance {
                    class: cls.clone(),
                    fields: RefCell::new(HashMap::new()),
                });
                let inst_val = VmValue::Instance(instance.clone());
                // Call __init__ if it exists.
                if let Some(init) = self.find_method(&cls, "__init__") {
                    let mut init_args = vec![inst_val.clone()];
                    init_args.extend_from_slice(args);
                    self.call_closure(init, &init_args, kwargs)?;
                }
                Ok(inst_val)
            }
            VmValue::BuiltinFn(name) => self.call_builtin(&name, args, kwargs),
            other => Err(self.err(&format!("'{}' is not callable", other.type_name()))),
        }
    }

    fn call_closure(
        &mut self,
        closure: Rc<VmClosure>,
        args: &[VmValue],
        kwargs: &[(String, VmValue)],
    ) -> Result<VmValue, String> {
        let params = &closure.proto.params;
        let local_count = closure.proto.local_count;
        let base = self.stack.len();

        // Extend stack to hold all locals.
        self.stack.resize(base + local_count.max(params.len()), VmValue::Nil);

        // Bind parameters.
        let mut pos_idx = 0usize;
        for (i, param) in params.iter().enumerate() {
            if param.is_vararg {
                // Collect remaining positional args.
                let rest: Vec<VmValue> = args[pos_idx..].to_vec();
                self.stack[base + i] = VmValue::List(Rc::new(RefCell::new(rest)));
                pos_idx = args.len();
            } else if param.is_kwarg {
                // Collect all kwargs into a dict.
                let mut d = VmDict::new();
                for (k, v) in kwargs {
                    d.set(VmValue::Str(k.clone()), v.clone());
                }
                self.stack[base + i] = VmValue::Dict(Rc::new(RefCell::new(d)));
            } else {
                // Check if provided as kwarg.
                if let Some((_, kv)) = kwargs.iter().find(|(k, _)| k == &param.name) {
                    self.stack[base + i] = kv.clone();
                } else if pos_idx < args.len() {
                    self.stack[base + i] = args[pos_idx].clone();
                    pos_idx += 1;
                } else if let Some(default_val) = closure.proto.defaults.get(i).and_then(|d| d.clone()) {
                    self.stack[base + i] = default_val;
                } else {
                    return Err(self.err(&format!("{}() missing argument '{}'", closure.proto.name, param.name)));
                }
            }
        }

        self.frames.push(CallFrame { closure, ip: 0, base });

        match self.execute() {
            Ok(v) => Ok(v),
            Err(e) => Err(e),
        }
    }

    fn find_method(&self, cls: &Rc<VmClass>, name: &str) -> Option<Rc<VmClosure>> {
        if let Some(m) = cls.methods.get(name) {
            return Some(m.clone());
        }
        if let Some(parent) = &cls.parent {
            return self.find_method(parent, name);
        }
        None
    }

    // ── Attribute access ──────────────────────────────────────────────────────

    fn get_attr(&self, obj: VmValue, name: &str) -> Result<VmValue, String> {
        match &obj {
            VmValue::Instance(inst) => {
                // Fields take priority.
                if let Some(v) = inst.fields.borrow().get(name).cloned() {
                    return Ok(v);
                }
                // Methods.
                if let Some(m) = self.find_method(&inst.class, name) {
                    return Ok(VmValue::BoundMethod(Rc::new(VmBoundMethod {
                        receiver: inst.clone(),
                        method: m,
                    })));
                }
                // Class variables (inherited).
                if let Some(v) = inst.class.class_vars.borrow().get(name).cloned() {
                    return Ok(v);
                }
                Err(self.err(&format!("'{}' object has no attribute '{}'", inst.class.name, name)))
            }
            VmValue::Class(cls) => {
                // Check class variables first.
                if let Some(v) = cls.class_vars.borrow().get(name).cloned() {
                    return Ok(v);
                }
                if let Some(m) = self.find_method(cls, name) {
                    return Ok(VmValue::Closure(m));
                }
                Err(self.err(&format!("class '{}' has no attribute '{}'", cls.name, name)))
            }
            VmValue::Super(sup) => {
                if let Some(m) = self.find_method(&sup.parent, name) {
                    return Ok(VmValue::BoundMethod(Rc::new(VmBoundMethod {
                        receiver: sup.instance.clone(),
                        method: m,
                    })));
                }
                Err(self.err(&format!("super has no method '{}'", name)))
            }
            VmValue::List(v) => self.list_method(obj.clone(), v.clone(), name),
            VmValue::Str(s) => self.str_method(obj.clone(), s.clone(), name),
            VmValue::Dict(d) => {
                // First check if `name` is a key in the dict (module namespace access).
                let key_val = {
                    let db = d.borrow();
                    let key_str = VmValue::Str(name.to_string());
                    db.keys
                        .iter()
                        .position(|k| vm_eq(k, &key_str))
                        .map(|i| db.vals[i].clone())
                };
                if let Some(v) = key_val {
                    return Ok(v);
                }
                self.dict_method(obj.clone(), d.clone(), name)
            }
            VmValue::File(fh) => self.file_method(obj.clone(), fh.clone(), name),
            VmValue::Socket(_) => self.vm_socket_method(obj.clone(), name),
            other => Err(self.err(&format!("'{}' has no attribute '{}'", other.type_name(), name))),
        }
    }

    fn set_attr(&self, obj: VmValue, name: &str, val: VmValue) -> Result<(), String> {
        match obj {
            VmValue::Instance(inst) => {
                inst.fields.borrow_mut().insert(name.to_string(), val);
                Ok(())
            }
            VmValue::Class(cls) => {
                match val {
                    VmValue::Closure(c) => {
                        unsafe {
                            let cls_ptr = Rc::as_ptr(&cls) as *mut VmClass;
                            (*cls_ptr).methods.insert(name.to_string(), c);
                        }
                        Ok(())
                    }
                    other => {
                        // Class variable (non-method)
                        cls.class_vars.borrow_mut().insert(name.to_string(), other);
                        Ok(())
                    }
                }
            }
            other => Err(self.err(&format!("cannot set attribute on '{}'", other.type_name()))),
        }
    }

    // ── Subscript ─────────────────────────────────────────────────────────────

    fn get_item(&self, obj: VmValue, idx: VmValue) -> Result<VmValue, String> {
        match (&obj, &idx) {
            (VmValue::List(v), VmValue::Int(i)) => {
                let v = v.borrow();
                let i = self.normalize_index(*i, v.len())?;
                Ok(v[i].clone())
            }
            (VmValue::Tuple(t), VmValue::Int(i)) => {
                let i = self.normalize_index(*i, t.len())?;
                Ok(t[i].clone())
            }
            (VmValue::Str(s), VmValue::Int(i)) => {
                let chars: Vec<char> = s.chars().collect();
                let i = self.normalize_index(*i, chars.len())?;
                Ok(VmValue::Str(chars[i].to_string()))
            }
            (VmValue::Dict(d), _) => d
                .borrow()
                .get(&idx)
                .ok_or_else(|| self.err(&format!("key not found: {}", idx))),
            _ => Err(self.err(&format!("cannot index {} with {}", obj.type_name(), idx.type_name()))),
        }
    }

    fn set_item(&self, obj: VmValue, idx: VmValue, val: VmValue) -> Result<(), String> {
        match (&obj, &idx) {
            (VmValue::List(v), VmValue::Int(i)) => {
                let mut v = v.borrow_mut();
                let i = self.normalize_index(*i, v.len())?;
                v[i] = val;
                Ok(())
            }
            (VmValue::Dict(d), _) => {
                d.borrow_mut().set(idx, val);
                Ok(())
            }
            _ => Err(self.err(&format!(
                "cannot index-assign {} with {}",
                obj.type_name(),
                idx.type_name()
            ))),
        }
    }

    fn get_slice(&self, obj: VmValue, start: VmValue, stop: VmValue) -> Result<VmValue, String> {
        let to_opt = |v: VmValue| match v {
            VmValue::Nil => None,
            VmValue::Int(n) => Some(n),
            _ => None,
        };
        let s = to_opt(start);
        let e = to_opt(stop);

        match obj {
            VmValue::List(v) => {
                let v = v.borrow();
                let len = v.len() as i64;
                let start = resolve_slice_idx(s, len, 0);
                let stop = resolve_slice_idx(e, len, len);
                let items = if start >= stop {
                    vec![]
                } else {
                    v[start as usize..stop as usize].to_vec()
                };
                Ok(VmValue::List(Rc::new(RefCell::new(items))))
            }
            VmValue::Str(s_str) => {
                let chars: Vec<char> = s_str.chars().collect();
                let len = chars.len() as i64;
                let start = resolve_slice_idx(s, len, 0);
                let stop = resolve_slice_idx(e, len, len);
                let slice: String = if start >= stop {
                    String::new()
                } else {
                    chars[start as usize..stop as usize].iter().collect()
                };
                Ok(VmValue::Str(slice))
            }
            VmValue::Tuple(t) => {
                let len = t.len() as i64;
                let start = resolve_slice_idx(s, len, 0);
                let stop = resolve_slice_idx(e, len, len);
                let items = if start >= stop {
                    vec![]
                } else {
                    t[start as usize..stop as usize].to_vec()
                };
                Ok(VmValue::Tuple(Rc::new(items)))
            }
            other => Err(self.err(&format!("cannot slice {}", other.type_name()))),
        }
    }

    fn normalize_index(&self, i: i64, len: usize) -> Result<usize, String> {
        let len = len as i64;
        let i = if i < 0 { len + i } else { i };
        if i < 0 || i >= len {
            Err(self.err(&format!("index {} out of range (len {})", i, len)))
        } else {
            Ok(i as usize)
        }
    }

    // ── Iterator construction ─────────────────────────────────────────────────

    fn make_iter(&self, obj: VmValue) -> Result<VmIter, String> {
        match obj {
            VmValue::List(v) => Ok(VmIter::List { items: v, idx: 0 }),
            VmValue::Tuple(t) => Ok(VmIter::Tuple { items: t, idx: 0 }),
            VmValue::Str(s) => Ok(VmIter::Str {
                chars: s.chars().collect(),
                idx: 0,
            }),
            VmValue::Dict(d) => Ok(VmIter::DictKeys { dict: d, idx: 0 }),
            VmValue::Iter(it) => Ok(match Rc::try_unwrap(it) {
                Ok(cell) => cell.into_inner(),
                Err(rc) => {
                    // Clone the iterator state.
                    match &*rc.borrow() {
                        VmIter::List { items, idx } => VmIter::List {
                            items: items.clone(),
                            idx: *idx,
                        },
                        VmIter::Tuple { items, idx } => VmIter::Tuple {
                            items: items.clone(),
                            idx: *idx,
                        },
                        VmIter::Str { chars, idx } => VmIter::Str {
                            chars: chars.clone(),
                            idx: *idx,
                        },
                        VmIter::Range { current, stop, step } => VmIter::Range {
                            current: *current,
                            stop: *stop,
                            step: *step,
                        },
                        VmIter::DictKeys { dict, idx } => VmIter::DictKeys {
                            dict: dict.clone(),
                            idx: *idx,
                        },
                    }
                }
            }),
            other => Err(self.err(&format!("'{}' is not iterable", other.type_name()))),
        }
    }

    fn to_iter_vec(&self, obj: VmValue) -> Result<Vec<VmValue>, String> {
        match obj {
            VmValue::List(v) => Ok(v.borrow().clone()),
            VmValue::Tuple(t) => Ok(t.as_ref().clone()),
            VmValue::Str(s) => Ok(s.chars().map(|c| VmValue::Str(c.to_string())).collect()),
            VmValue::Dict(d) => Ok(d.borrow().keys.clone()),
            other => Err(self.err(&format!("'{}' is not iterable", other.type_name()))),
        }
    }

    // ── Arithmetic helpers ────────────────────────────────────────────────────

    fn add(&self, l: VmValue, r: VmValue) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => Ok(VmValue::Int(a.wrapping_add(b))),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a + b)),
            (VmValue::Int(a), VmValue::Float(b)) => Ok(VmValue::Float(a as f64 + b)),
            (VmValue::Float(a), VmValue::Int(b)) => Ok(VmValue::Float(a + b as f64)),
            (VmValue::Str(a), VmValue::Str(b)) => Ok(VmValue::Str(a + &b)),
            (VmValue::List(a), VmValue::List(b)) => {
                let mut v = a.borrow().clone();
                v.extend_from_slice(&b.borrow());
                Ok(VmValue::List(Rc::new(RefCell::new(v))))
            }
            (l, r) => Err(self.err(&format!("cannot add {} and {}", l.type_name(), r.type_name()))),
        }
    }

    fn arith(&self, l: VmValue, r: VmValue, op: &str) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => Ok(VmValue::Int(match op {
                "-" => a.wrapping_sub(b),
                _ => unreachable!(),
            })),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(match op {
                "-" => a - b,
                _ => unreachable!(),
            })),
            (VmValue::Int(a), VmValue::Float(b)) => Ok(VmValue::Float(match op {
                "-" => a as f64 - b,
                _ => unreachable!(),
            })),
            (VmValue::Float(a), VmValue::Int(b)) => Ok(VmValue::Float(match op {
                "-" => a - b as f64,
                _ => unreachable!(),
            })),
            (l, r) => Err(self.err(&format!("cannot {} {} and {}", op, l.type_name(), r.type_name()))),
        }
    }

    fn mul(&self, l: VmValue, r: VmValue) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => Ok(VmValue::Int(a.wrapping_mul(b))),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a * b)),
            (VmValue::Int(a), VmValue::Float(b)) => Ok(VmValue::Float(a as f64 * b)),
            (VmValue::Float(a), VmValue::Int(b)) => Ok(VmValue::Float(a * b as f64)),
            (VmValue::Str(s), VmValue::Int(n)) | (VmValue::Int(n), VmValue::Str(s)) => {
                Ok(VmValue::Str(s.repeat(n.max(0) as usize)))
            }
            (VmValue::List(v), VmValue::Int(n)) => {
                let v = v.borrow();
                let mut result = Vec::new();
                for _ in 0..n.max(0) {
                    result.extend_from_slice(&v);
                }
                Ok(VmValue::List(Rc::new(RefCell::new(result))))
            }
            (l, r) => Err(self.err(&format!("cannot multiply {} and {}", l.type_name(), r.type_name()))),
        }
    }

    fn div(&self, l: VmValue, r: VmValue) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => {
                if b == 0 {
                    return Err(self.err("division by zero"));
                }
                Ok(VmValue::Float(a as f64 / b as f64))
            }
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a / b)),
            (VmValue::Int(a), VmValue::Float(b)) => Ok(VmValue::Float(a as f64 / b)),
            (VmValue::Float(a), VmValue::Int(b)) => Ok(VmValue::Float(a / b as f64)),
            (l, r) => Err(self.err(&format!("cannot divide {} and {}", l.type_name(), r.type_name()))),
        }
    }

    fn modulo(&self, l: VmValue, r: VmValue) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => {
                if b == 0 {
                    return Err(self.err("modulo by zero"));
                }
                Ok(VmValue::Int(a.rem_euclid(b)))
            }
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a % b)),
            (VmValue::Int(a), VmValue::Float(b)) => Ok(VmValue::Float(a as f64 % b)),
            (VmValue::Float(a), VmValue::Int(b)) => Ok(VmValue::Float(a % b as f64)),
            (VmValue::Str(fmt_str), r) => self.str_format(&fmt_str, r),
            (l, r) => Err(self.err(&format!("cannot mod {} and {}", l.type_name(), r.type_name()))),
        }
    }

    fn pow(&self, l: VmValue, r: VmValue) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => {
                if b >= 0 {
                    Ok(VmValue::Int(a.pow(b as u32)))
                } else {
                    Ok(VmValue::Float((a as f64).powi(b as i32)))
                }
            }
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a.powf(b))),
            (VmValue::Int(a), VmValue::Float(b)) => Ok(VmValue::Float((a as f64).powf(b))),
            (VmValue::Float(a), VmValue::Int(b)) => Ok(VmValue::Float(a.powi(b as i32))),
            (l, r) => Err(self.err(&format!("cannot pow {} and {}", l.type_name(), r.type_name()))),
        }
    }

    fn floor_div(&self, l: VmValue, r: VmValue) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => {
                if b == 0 {
                    return Err(self.err("floor division by zero"));
                }
                Ok(VmValue::Int(a.div_euclid(b)))
            }
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float((a / b).floor())),
            (VmValue::Int(a), VmValue::Float(b)) => Ok(VmValue::Float((a as f64 / b).floor())),
            (VmValue::Float(a), VmValue::Int(b)) => Ok(VmValue::Float((a / b as f64).floor())),
            (l, r) => Err(self.err(&format!("cannot floor-div {} and {}", l.type_name(), r.type_name()))),
        }
    }

    fn cmp_op(&self, l: VmValue, r: VmValue, op: &str) -> Result<VmValue, String> {
        let result = match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                ">=" => a >= b,
                _ => unreachable!(),
            },
            (VmValue::Float(a), VmValue::Float(b)) => match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                ">=" => a >= b,
                _ => unreachable!(),
            },
            (VmValue::Int(a), VmValue::Float(b)) => match op {
                "<" => (a as f64) < b,
                "<=" => (a as f64) <= b,
                ">" => (a as f64) > b,
                ">=" => (a as f64) >= b,
                _ => unreachable!(),
            },
            (VmValue::Float(a), VmValue::Int(b)) => match op {
                "<" => a < b as f64,
                "<=" => a <= b as f64,
                ">" => a > b as f64,
                ">=" => a >= b as f64,
                _ => unreachable!(),
            },
            (VmValue::Str(a), VmValue::Str(b)) => match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                ">=" => a >= b,
                _ => unreachable!(),
            },
            (l, r) => return Err(self.err(&format!("cannot compare {} and {}", l.type_name(), r.type_name()))),
        };
        Ok(VmValue::Bool(result))
    }

    fn bitop(&self, l: VmValue, r: VmValue, op: &str) -> Result<VmValue, String> {
        match (l, r) {
            (VmValue::Int(a), VmValue::Int(b)) => Ok(VmValue::Int(match op {
                "&" => a & b,
                "|" => a | b,
                "^" => a ^ b,
                "<<" => a << (b as u32),
                ">>" => a >> (b as u32),
                _ => unreachable!(),
            })),
            (l, r) => Err(self.err(&format!(
                "bitwise {} requires ints, got {} and {}",
                op,
                l.type_name(),
                r.type_name()
            ))),
        }
    }

    fn contains(&self, container: &VmValue, item: &VmValue) -> Result<bool, String> {
        match container {
            VmValue::List(v) => Ok(v.borrow().iter().any(|x| vm_eq(x, item))),
            VmValue::Tuple(t) => Ok(t.iter().any(|x| vm_eq(x, item))),
            VmValue::Str(s) => {
                if let VmValue::Str(sub) = item {
                    Ok(s.contains(sub.as_str()))
                } else {
                    Err(self.err("'in' on str requires str"))
                }
            }
            VmValue::Dict(d) => Ok(d.borrow().contains(item)),
            other => Err(self.err(&format!("'in' not supported for {}", other.type_name()))),
        }
    }

    fn str_format(&self, fmt: &str, arg: VmValue) -> Result<VmValue, String> {
        // Simple %s / %d / %f formatting.
        let replacement = arg.to_string();
        Ok(VmValue::Str(
            fmt.replacen("%s", &replacement, 1)
                .replacen("%d", &replacement, 1)
                .replacen("%f", &replacement, 1),
        ))
    }

    // ── Built-in methods on primitive types ───────────────────────────────────

    fn list_method(&self, receiver: VmValue, _list: Rc<RefCell<Vec<VmValue>>>, name: &str) -> Result<VmValue, String> {
        match name {
            "append" | "pop" | "sort" | "reverse" | "extend" | "insert" | "remove" | "index" | "count" | "clear"
            | "copy" => Ok(VmValue::BoundBuiltin(Box::new(receiver), format!("list.{}", name))),
            _ => Err(self.err(&format!("list has no method '{}'", name))),
        }
    }

    fn str_method(&self, receiver: VmValue, s: String, name: &str) -> Result<VmValue, String> {
        match name {
            "upper" | "lower" | "strip" | "lstrip" | "rstrip" | "split" | "replace" | "find" | "count"
            | "startswith" | "endswith" | "join" | "format" | "title" | "capitalize" | "encode" | "isdigit"
            | "isalpha" | "isspace" | "zfill" => {
                // Embed the string value in the builtin name so we can recover it at call time.
                Ok(VmValue::BoundBuiltin(Box::new(receiver), format!("str.{}:{}", name, s)))
            }
            _ => Err(self.err(&format!("str has no method '{}'", name))),
        }
    }

    fn dict_method(&self, receiver: VmValue, _dict: Rc<RefCell<VmDict>>, name: &str) -> Result<VmValue, String> {
        match name {
            "keys" | "values" | "items" | "get" | "pop" | "update" | "clear" | "copy" | "contains" | "has_key" => {
                Ok(VmValue::BoundBuiltin(Box::new(receiver), format!("dict.{}", name)))
            }
            _ => Err(self.err(&format!("dict has no method '{}'", name))),
        }
    }

    fn file_method(&self, receiver: VmValue, _fh: Rc<RefCell<VmFile>>, name: &str) -> Result<VmValue, String> {
        match name {
            "read" | "readline" | "readlines" | "write" | "close" | "seek" | "tell" => {
                Ok(VmValue::BoundBuiltin(Box::new(receiver), format!("file.{}", name)))
            }
            // Context manager protocol: __enter__ returns self, __exit__ closes the file.
            "__enter__" => Ok(VmValue::BoundBuiltin(Box::new(receiver), "file.__enter__".to_string())),
            "__exit__" => Ok(VmValue::BoundBuiltin(Box::new(receiver), "file.__exit__".to_string())),
            _ => Err(self.err(&format!("file has no method '{}'", name))),
        }
    }

    // ── Built-in function dispatch ────────────────────────────────────────────

    fn call_builtin(&mut self, name: &str, args: &[VmValue], kwargs: &[(String, VmValue)]) -> Result<VmValue, String> {
        // Handle namespaced method builtins first.
        if let Some(_f) = name.strip_prefix("ffi.") {
            return Err(self.err("FFI is only supported in the tree-walk interpreter (run without --vm)"));
        }
        if let Some(rest) = name.strip_prefix("subprocess.") {
            return self.call_subprocess_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("argparse.") {
            return self.call_argparse_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("csv.") {
            return self.call_csv_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("datetime.") {
            return self.call_datetime_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("hashlib.") {
            return self.call_hashlib_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("toml.") {
            return self.call_toml_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("yaml.") {
            return self.call_yaml_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("sqlite.") {
            return self.call_sqlite_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("http.") {
            return self.call_http_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("term.") {
            return self.call_term_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("test.") {
            return self.call_test_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("logging.") {
            return self.call_logging_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("path.") {
            return self.call_path_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("platform.") {
            return self.call_platform_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("core.") {
            return self.call_core_module(rest, args);
        }
        if let Some(rest) = name.strip_prefix("listmod.") {
            return self.call_list_module(rest, args, kwargs);
        }
        if let Some(rest) = name.strip_prefix("stringmod.") {
            return self.call_string_module(rest, args, kwargs);
        }
        if let Some(rest) = name.strip_prefix("list.") {
            return self.call_list_method(rest, args, kwargs);
        }
        if let Some(rest) = name.strip_prefix("dict.") {
            return self.call_dict_method(rest, args, kwargs);
        }
        if let Some(rest) = name.strip_prefix("str.") {
            return self.call_str_method(rest, args, kwargs);
        }
        if let Some(rest) = name.strip_prefix("file.") {
            return self.call_file_method(rest, args, kwargs);
        }
        if let Some(rest) = name.strip_prefix("socket.") {
            return self.call_socket_module(rest, args);
        }

        match name {
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
            "print" => {
                let sep = kwargs
                    .iter()
                    .find(|(k, _)| k == "sep")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_else(|| " ".to_string());
                let end = kwargs
                    .iter()
                    .find(|(k, _)| k == "end")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_else(|| "\n".to_string());
                let s: Vec<String> = args.iter().map(|v| v.to_string()).collect();
                print!("{}{}", s.join(&sep), end);
                use std::io::Write;
                std::io::stdout().flush().ok();
                Ok(VmValue::Nil)
            }
            "len" => {
                let v = args.first().ok_or_else(|| self.err("len() requires 1 argument"))?;
                Ok(VmValue::Int(self.vm_len(v)? as i64))
            }
            "range" => match args {
                [VmValue::Int(stop)] => {
                    let items: Vec<VmValue> = (0..*stop).map(VmValue::Int).collect();
                    Ok(VmValue::List(Rc::new(RefCell::new(items))))
                }
                [VmValue::Int(start), VmValue::Int(stop)] => {
                    let items: Vec<VmValue> = (*start..*stop).map(VmValue::Int).collect();
                    Ok(VmValue::List(Rc::new(RefCell::new(items))))
                }
                [VmValue::Int(start), VmValue::Int(stop), VmValue::Int(step)] => {
                    let mut items = Vec::new();
                    let mut i = *start;
                    while if *step > 0 { i < *stop } else { i > *stop } {
                        items.push(VmValue::Int(i));
                        i += step;
                    }
                    Ok(VmValue::List(Rc::new(RefCell::new(items))))
                }
                _ => Err(self.err("range() requires 1–3 int arguments")),
            },
            "str" => {
                let v = args.first().unwrap_or(&VmValue::Nil).clone();
                // Call __str__ if the object has one.
                if let VmValue::Instance(ref inst) = v {
                    if let Some(m) = self.find_method(&inst.class, "__str__") {
                        let result = self.call_closure(m, &[v.clone()], &[])?;
                        return Ok(result);
                    }
                }
                Ok(VmValue::Str(v.to_string()))
            }
            "int" => {
                let v = args.first().ok_or_else(|| self.err("int() requires 1 argument"))?;
                Ok(VmValue::Int(self.coerce_to_int(v)?))
            }
            "i8" => {
                let v = args.first().ok_or_else(|| self.err("i8() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_signed(self.coerce_to_int(v)?, 8)))
            }
            "u8" => {
                let v = args.first().ok_or_else(|| self.err("u8() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_unsigned(self.coerce_to_int(v)?, 8)))
            }
            "i16" => {
                let v = args.first().ok_or_else(|| self.err("i16() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_signed(self.coerce_to_int(v)?, 16)))
            }
            "u16" => {
                let v = args.first().ok_or_else(|| self.err("u16() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_unsigned(self.coerce_to_int(v)?, 16)))
            }
            "i32" => {
                let v = args.first().ok_or_else(|| self.err("i32() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_signed(self.coerce_to_int(v)?, 32)))
            }
            "u32" => {
                let v = args.first().ok_or_else(|| self.err("u32() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_unsigned(self.coerce_to_int(v)?, 32)))
            }
            "i64" => {
                let v = args.first().ok_or_else(|| self.err("i64() requires 1 argument"))?;
                Ok(VmValue::Int(self.coerce_to_int(v)?))
            }
            "isize" => {
                let v = args.first().ok_or_else(|| self.err("isize() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_signed(self.coerce_to_int(v)?, COOL_POINTER_BITS)))
            }
            "usize" => {
                let v = args.first().ok_or_else(|| self.err("usize() requires 1 argument"))?;
                Ok(VmValue::Int(wrap_unsigned(self.coerce_to_int(v)?, COOL_POINTER_BITS)))
            }
            "word_bits" => {
                if !args.is_empty() {
                    return Err(self.err("word_bits() takes no arguments"));
                }
                Ok(VmValue::Int(COOL_POINTER_BITS as i64))
            }
            "word_bytes" => {
                if !args.is_empty() {
                    return Err(self.err("word_bytes() takes no arguments"));
                }
                Ok(VmValue::Int(COOL_POINTER_BYTES))
            }
            "float" => {
                let v = args.first().ok_or_else(|| self.err("float() requires 1 argument"))?;
                match v {
                    VmValue::Float(f) => Ok(VmValue::Float(*f)),
                    VmValue::Int(n) => Ok(VmValue::Float(*n as f64)),
                    VmValue::Str(s) => s
                        .trim()
                        .parse::<f64>()
                        .map(VmValue::Float)
                        .map_err(|_| self.err(&format!("invalid float: '{}'", s))),
                    other => Err(self.err(&format!("cannot convert {} to float", other.type_name()))),
                }
            }
            "bool" => {
                let v = args.first().unwrap_or(&VmValue::Nil);
                Ok(VmValue::Bool(v.is_truthy()))
            }
            "type" => {
                let v = args.first().unwrap_or(&VmValue::Nil);
                Ok(VmValue::Str(v.type_name().to_string()))
            }
            "repr" => {
                let v = args.first().unwrap_or(&VmValue::Nil);
                Ok(VmValue::Str(vm_repr(v)))
            }
            "abs" => match args.first() {
                Some(VmValue::Int(n)) => Ok(VmValue::Int(n.abs())),
                Some(VmValue::Float(f)) => Ok(VmValue::Float(f.abs())),
                _ => Err(self.err("abs() requires a number")),
            },
            "round" => {
                let ndigits = args
                    .get(1)
                    .and_then(|v| if let VmValue::Int(n) = v { Some(*n) } else { None })
                    .unwrap_or(0);
                match args.first() {
                    Some(VmValue::Int(n)) => Ok(VmValue::Int(*n)),
                    Some(VmValue::Float(f)) => {
                        if ndigits == 0 {
                            Ok(VmValue::Int(f.round() as i64))
                        } else {
                            let factor = 10f64.powi(ndigits as i32);
                            Ok(VmValue::Float((f * factor).round() / factor))
                        }
                    }
                    _ => Err(self.err("round() requires a number")),
                }
            }
            "min" => self.builtin_min_max(args, false),
            "max" => self.builtin_min_max(args, true),
            "sum" => {
                let items = match args.first() {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    Some(VmValue::Tuple(t)) => t.as_ref().clone(),
                    _ => return Err(self.err("sum() requires a list or tuple")),
                };
                let mut total: i64 = 0;
                let mut ftotal: f64 = 0.0;
                let mut is_float = false;
                for item in &items {
                    match item {
                        VmValue::Int(n) => {
                            total += n;
                            ftotal += *n as f64;
                        }
                        VmValue::Float(f) => {
                            ftotal += f;
                            is_float = true;
                        }
                        _ => return Err(self.err("sum() requires a list or tuple of numbers")),
                    }
                }
                if is_float {
                    Ok(VmValue::Float(ftotal))
                } else {
                    Ok(VmValue::Int(total))
                }
            }
            "any" => {
                let items = match args.first() {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    Some(VmValue::Tuple(t)) => t.as_ref().clone(),
                    _ => return Err(self.err("any() requires a list")),
                };
                for item in &items {
                    if item.is_truthy() {
                        return Ok(VmValue::Bool(true));
                    }
                }
                Ok(VmValue::Bool(false))
            }
            "all" => {
                let items = match args.first() {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    Some(VmValue::Tuple(t)) => t.as_ref().clone(),
                    _ => return Err(self.err("all() requires a list")),
                };
                for item in &items {
                    if !item.is_truthy() {
                        return Ok(VmValue::Bool(false));
                    }
                }
                Ok(VmValue::Bool(true))
            }
            "sorted" => {
                let items = match args.first() {
                    Some(v) => self.to_iter_vec(v.clone())?,
                    None => return Err(self.err("sorted() requires an argument")),
                };
                let mut v = items;
                let reverse = kwargs
                    .iter()
                    .find(|(k, _)| k == "reverse")
                    .map(|(_, v)| v.is_truthy())
                    .unwrap_or(false);
                v.sort_by(|a, b| vm_cmp_order(a, b));
                if reverse {
                    v.reverse();
                }
                Ok(VmValue::List(Rc::new(RefCell::new(v))))
            }
            "reversed" => {
                let items = self.to_iter_vec(
                    args.first()
                        .cloned()
                        .ok_or_else(|| self.err("reversed() requires an argument"))?,
                )?;
                let mut v = items;
                v.reverse();
                Ok(VmValue::List(Rc::new(RefCell::new(v))))
            }
            "enumerate" => {
                let items = self.to_iter_vec(
                    args.first()
                        .cloned()
                        .ok_or_else(|| self.err("enumerate() requires an argument"))?,
                )?;
                let start = if let Some(VmValue::Int(n)) = args.get(1) { *n } else { 0 };
                let result: Vec<VmValue> = items
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| VmValue::Tuple(Rc::new(vec![VmValue::Int(start + i as i64), v])))
                    .collect();
                Ok(VmValue::List(Rc::new(RefCell::new(result))))
            }
            "zip" => {
                if args.is_empty() {
                    return Ok(VmValue::List(Rc::new(RefCell::new(vec![]))));
                }
                let iters: Result<Vec<Vec<VmValue>>, String> =
                    args.iter().map(|a| self.to_iter_vec(a.clone())).collect();
                let iters = iters?;
                let len = iters.iter().map(|v| v.len()).min().unwrap_or(0);
                let result: Vec<VmValue> = (0..len)
                    .map(|i| VmValue::Tuple(Rc::new(iters.iter().map(|v| v[i].clone()).collect())))
                    .collect();
                Ok(VmValue::List(Rc::new(RefCell::new(result))))
            }
            "map" => {
                let func = args
                    .first()
                    .cloned()
                    .ok_or_else(|| self.err("map() requires a function"))?;
                let items = self.to_iter_vec(
                    args.get(1)
                        .cloned()
                        .ok_or_else(|| self.err("map() requires an iterable"))?,
                )?;
                let mut result = Vec::new();
                for item in items {
                    let v = self.call_value_direct(func.clone(), &[item], &[])?;
                    result.push(v);
                }
                Ok(VmValue::List(Rc::new(RefCell::new(result))))
            }
            "filter" => {
                let func = args
                    .first()
                    .cloned()
                    .ok_or_else(|| self.err("filter() requires a function"))?;
                let items = self.to_iter_vec(
                    args.get(1)
                        .cloned()
                        .ok_or_else(|| self.err("filter() requires an iterable"))?,
                )?;
                let mut result = Vec::new();
                for item in items {
                    let v = self.call_value_direct(func.clone(), &[item.clone()], &[])?;
                    if v.is_truthy() {
                        result.push(item);
                    }
                }
                Ok(VmValue::List(Rc::new(RefCell::new(result))))
            }
            "isinstance" => {
                let obj = args.first().ok_or_else(|| self.err("isinstance() requires 2 args"))?;
                let cls = args.get(1).ok_or_else(|| self.err("isinstance() requires 2 args"))?;
                match (obj, cls) {
                    (VmValue::Instance(inst), VmValue::Class(c)) => {
                        Ok(VmValue::Bool(self.is_instance_of(&inst.class, c)))
                    }
                    _ => Ok(VmValue::Bool(false)),
                }
            }
            "hasattr" => {
                let obj = args
                    .first()
                    .cloned()
                    .ok_or_else(|| self.err("hasattr() requires 2 args"))?;
                let attr = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("hasattr() attribute must be a string")),
                };
                Ok(VmValue::Bool(self.get_attr(obj, &attr).is_ok()))
            }
            "getattr" => {
                let obj = args
                    .first()
                    .cloned()
                    .ok_or_else(|| self.err("getattr() requires 2 args"))?;
                let attr = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("getattr() attribute must be a string")),
                };
                match (self.get_attr(obj, &attr), args.get(2)) {
                    (Ok(v), _) => Ok(v),
                    (Err(_), Some(default)) => Ok(default.clone()),
                    (Err(e), None) => Err(e),
                }
            }
            "list" => match args.first() {
                None => Ok(VmValue::List(Rc::new(RefCell::new(vec![])))),
                Some(v) => {
                    let items = self.to_iter_vec(v.clone())?;
                    Ok(VmValue::List(Rc::new(RefCell::new(items))))
                }
            },
            "tuple" => match args.first() {
                None => Ok(VmValue::Tuple(Rc::new(vec![]))),
                Some(v) => {
                    let items = self.to_iter_vec(v.clone())?;
                    Ok(VmValue::Tuple(Rc::new(items)))
                }
            },
            "dict" => {
                let d = VmDict::new();
                Ok(VmValue::Dict(Rc::new(RefCell::new(d))))
            }
            "set" => {
                // Return a list with unique items (set is not fully implemented).
                let items = match args.first() {
                    None => vec![],
                    Some(v) => self.to_iter_vec(v.clone())?,
                };
                let mut unique: Vec<VmValue> = Vec::new();
                for item in items {
                    if !unique.iter().any(|x| vm_eq(x, &item)) {
                        unique.push(item);
                    }
                }
                Ok(VmValue::List(Rc::new(RefCell::new(unique))))
            }
            "input" => {
                use std::io::{self, BufRead, Write};
                let prompt = args.first().map(|v| v.to_string()).unwrap_or_default();
                print!("{}", prompt);
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin().lock().read_line(&mut line).ok();
                Ok(VmValue::Str(
                    line.trim_end_matches('\n').trim_end_matches('\r').to_string(),
                ))
            }
            "exit" => {
                let code = match args.first() {
                    Some(VmValue::Int(n)) => *n as i32,
                    _ => 0,
                };
                std::process::exit(code);
            }
            "open" => {
                let path = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("open() requires a path string")),
                };
                let mode = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    None => "r".to_string(),
                    _ => return Err(self.err("open() mode must be a string")),
                };
                let full_path = self.source_dir.join(&path);
                match mode.as_str() {
                    "r" | "r+" => {
                        let content = std::fs::read_to_string(&full_path)
                            .map_err(|e| self.err(&format!("open '{}': {}", path, e)))?;
                        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
                        Ok(VmValue::File(Rc::new(RefCell::new(VmFile {
                            path,
                            mode,
                            content: lines,
                            line_pos: 0,
                            write_buf: String::new(),
                            closed: false,
                        }))))
                    }
                    "w" | "a" | "w+" | "a+" => Ok(VmValue::File(Rc::new(RefCell::new(VmFile {
                        path,
                        mode,
                        content: vec![],
                        line_pos: 0,
                        write_buf: String::new(),
                        closed: false,
                    })))),
                    _ => Err(self.err(&format!("unsupported file mode '{}'", mode))),
                }
            }
            "super" => {
                // super() returns a Super value for the current instance.
                // Look up `self` in the current frame.
                let self_val = self
                    .stack
                    .get(self.frames.last().map(|f| f.base).unwrap_or(0))
                    .cloned()
                    .unwrap_or(VmValue::Nil);
                match self_val {
                    VmValue::Instance(inst) => {
                        let parent = inst
                            .class
                            .parent
                            .clone()
                            .ok_or_else(|| self.err("class has no parent"))?;
                        Ok(VmValue::Super(Rc::new(VmSuper { instance: inst, parent })))
                    }
                    _ => Err(self.err("super() called outside of method")),
                }
            }
            "runfile" => {
                let path = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("runfile() requires a path string")),
                };
                let extra_args: Vec<String> = match args.get(1) {
                    None => Vec::new(),
                    Some(VmValue::List(items)) => items.borrow().iter().map(|v| v.to_string()).collect(),
                    Some(VmValue::Tuple(items)) => items.iter().map(|v| v.to_string()).collect(),
                    Some(_) => return Err(self.err("runfile() 2nd argument must be a list or tuple of args")),
                };
                self.run_file(&path, &extra_args)
            }
            "__import_file__" => {
                let path = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("__import_file__ requires a string")),
                };
                self.import_file(&path)
            }
            "__import_module__" => {
                let module_name = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("__import_module__ requires a string")),
                };
                self.import_module(&module_name)
            }
            "__exc_matches__" => {
                // Check if an exception matches a type.
                let exc = args
                    .first()
                    .ok_or_else(|| self.err("__exc_matches__ requires 2 args"))?;
                let type_val = args.get(1).ok_or_else(|| self.err("__exc_matches__ requires 2 args"))?;
                let result = match (exc, type_val) {
                    (VmValue::Instance(inst), VmValue::Class(cls)) => self.is_instance_of(&inst.class, cls),
                    (VmValue::Str(_), VmValue::Class(cls)) => {
                        cls.name == "Exception" || cls.name == "ValueError" || cls.name == "TypeError"
                    }
                    (_, VmValue::Class(cls)) => cls.name == "Exception",
                    _ => false,
                };
                Ok(VmValue::Bool(result))
            }
            "set_completions" => Ok(VmValue::Nil),
            "eval" => {
                let src = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("eval() requires a string")),
                };
                self.eval_source(&src)
            }
            // ── math module ──────────────────────────────────────────────────
            _ if name.starts_with("math.") => {
                let fname = &name[5..];
                let n = match args.first() {
                    Some(VmValue::Int(i)) => *i as f64,
                    Some(VmValue::Float(f)) => *f,
                    _ if fname == "gcd" || fname == "lcm" || fname == "atan2" || fname == "hypot" || fname == "pow" => {
                        0.0
                    }
                    _ => return Err(self.err(&format!("math.{}() requires a number", fname))),
                };
                match fname {
                    "sqrt" => Ok(VmValue::Float(n.sqrt())),
                    "floor" => Ok(VmValue::Int(n.floor() as i64)),
                    "ceil" => Ok(VmValue::Int(n.ceil() as i64)),
                    "trunc" => Ok(VmValue::Int(n.trunc() as i64)),
                    "round" => {
                        let ndigits = args
                            .get(1)
                            .and_then(|v| if let VmValue::Int(i) = v { Some(*i) } else { None })
                            .unwrap_or(0);
                        if ndigits == 0 {
                            Ok(VmValue::Int(n.round() as i64))
                        } else {
                            let factor = 10f64.powi(ndigits as i32);
                            Ok(VmValue::Float((n * factor).round() / factor))
                        }
                    }
                    "abs" => match args.first() {
                        Some(VmValue::Int(v)) => Ok(VmValue::Int(v.abs())),
                        _ => Ok(VmValue::Float(n.abs())),
                    },
                    "exp" => Ok(VmValue::Float(n.exp())),
                    "exp2" => Ok(VmValue::Float(n.exp2())),
                    "sin" => Ok(VmValue::Float(n.sin())),
                    "cos" => Ok(VmValue::Float(n.cos())),
                    "tan" => Ok(VmValue::Float(n.tan())),
                    "asin" => Ok(VmValue::Float(n.asin())),
                    "acos" => Ok(VmValue::Float(n.acos())),
                    "atan" => Ok(VmValue::Float(n.atan())),
                    "sinh" => Ok(VmValue::Float(n.sinh())),
                    "cosh" => Ok(VmValue::Float(n.cosh())),
                    "tanh" => Ok(VmValue::Float(n.tanh())),
                    "degrees" => Ok(VmValue::Float(n.to_degrees())),
                    "radians" => Ok(VmValue::Float(n.to_radians())),
                    "isnan" => Ok(VmValue::Bool(n.is_nan())),
                    "isinf" => Ok(VmValue::Bool(n.is_infinite())),
                    "isfinite" => Ok(VmValue::Bool(n.is_finite())),
                    "log" => {
                        if args.len() >= 2 {
                            let base = match &args[1] {
                                VmValue::Int(i) => *i as f64,
                                VmValue::Float(f) => *f,
                                _ => return Err(self.err("math.log base must be a number")),
                            };
                            Ok(VmValue::Float(n.log(base)))
                        } else {
                            Ok(VmValue::Float(n.ln()))
                        }
                    }
                    "log2" => Ok(VmValue::Float(n.log2())),
                    "log10" => Ok(VmValue::Float(n.log10())),
                    "atan2" => {
                        let y = n;
                        let x = match args.get(1) {
                            Some(VmValue::Int(i)) => *i as f64,
                            Some(VmValue::Float(f)) => *f,
                            _ => return Err(self.err("math.atan2 requires two numbers")),
                        };
                        Ok(VmValue::Float(y.atan2(x)))
                    }
                    "pow" => {
                        let exp = match args.get(1) {
                            Some(VmValue::Int(i)) => *i as f64,
                            Some(VmValue::Float(f)) => *f,
                            _ => return Err(self.err("math.pow requires two numbers")),
                        };
                        Ok(VmValue::Float(n.powf(exp)))
                    }
                    "hypot" => {
                        let b = match args.get(1) {
                            Some(VmValue::Int(i)) => *i as f64,
                            Some(VmValue::Float(f)) => *f,
                            _ => return Err(self.err("math.hypot requires two numbers")),
                        };
                        Ok(VmValue::Float(n.hypot(b)))
                    }
                    "gcd" => {
                        let a = match args.first() {
                            Some(VmValue::Int(i)) => *i,
                            _ => return Err(self.err("math.gcd requires ints")),
                        };
                        let b = match args.get(1) {
                            Some(VmValue::Int(i)) => *i,
                            _ => return Err(self.err("math.gcd requires ints")),
                        };
                        fn gcd(a: i64, b: i64) -> i64 {
                            if b == 0 {
                                a.abs()
                            } else {
                                gcd(b, a % b)
                            }
                        }
                        Ok(VmValue::Int(gcd(a, b)))
                    }
                    "lcm" => {
                        let a = match args.first() {
                            Some(VmValue::Int(i)) => *i,
                            _ => return Err(self.err("math.lcm requires ints")),
                        };
                        let b = match args.get(1) {
                            Some(VmValue::Int(i)) => *i,
                            _ => return Err(self.err("math.lcm requires ints")),
                        };
                        fn gcd(a: i64, b: i64) -> i64 {
                            if b == 0 {
                                a.abs()
                            } else {
                                gcd(b, a % b)
                            }
                        }
                        Ok(VmValue::Int(if a == 0 || b == 0 { 0 } else { (a / gcd(a, b)) * b }))
                    }
                    "factorial" => {
                        let n = match args.first() {
                            Some(VmValue::Int(i)) => *i,
                            _ => return Err(self.err("math.factorial requires int")),
                        };
                        if n < 0 {
                            return Err(self.err("math.factorial: negative argument"));
                        }
                        Ok(VmValue::Int((1..=n).product()))
                    }
                    _ => Err(self.err(&format!("unknown math function '{}'", fname))),
                }
            }
            // ── os module ────────────────────────────────────────────────────
            _ if name.starts_with("os.") => {
                let fname = &name[3..];
                match fname {
                    "getcwd" => Ok(VmValue::Str(
                        std::env::current_dir()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )),
                    "listdir" => {
                        let path = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => ".".to_string(),
                        };
                        let entries: Vec<VmValue> = std::fs::read_dir(&path)
                            .map_err(|e| self.err(&e.to_string()))?
                            .filter_map(|e| e.ok())
                            .map(|e| VmValue::Str(e.file_name().to_string_lossy().to_string()))
                            .collect();
                        Ok(VmValue::List(Rc::new(RefCell::new(entries))))
                    }
                    "mkdir" => {
                        let path = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.mkdir requires a string")),
                        };
                        std::fs::create_dir_all(&path).map_err(|e| self.err(&e.to_string()))?;
                        Ok(VmValue::Nil)
                    }
                    "remove" => {
                        let path = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.remove requires a string")),
                        };
                        std::fs::remove_file(&path).map_err(|e| self.err(&e.to_string()))?;
                        Ok(VmValue::Nil)
                    }
                    "rename" => {
                        let src = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.rename requires strings")),
                        };
                        let dst = match args.get(1) {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.rename requires two strings")),
                        };
                        std::fs::rename(&src, &dst).map_err(|e| self.err(&e.to_string()))?;
                        Ok(VmValue::Nil)
                    }
                    "exists" => {
                        let path = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.exists requires a string")),
                        };
                        Ok(VmValue::Bool(std::path::Path::new(&path).exists()))
                    }
                    "isdir" => {
                        let path = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.isdir requires a string")),
                        };
                        Ok(VmValue::Bool(std::path::Path::new(&path).is_dir()))
                    }
                    "getenv" => {
                        let name = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.getenv requires a string")),
                        };
                        Ok(match std::env::var(&name) {
                            Ok(value) => VmValue::Str(value),
                            Err(_) => VmValue::Nil,
                        })
                    }
                    "popen" => {
                        let cmd = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("os.popen requires a string")),
                        };
                        let output = std::process::Command::new("sh")
                            .arg("-c")
                            .arg(&cmd)
                            .output()
                            .map_err(|e| self.err(&e.to_string()))?;
                        Ok(VmValue::Str(String::from_utf8_lossy(&output.stdout).to_string()))
                    }
                    "join" | "path" => {
                        let parts: Vec<String> = args.iter().map(|v| v.to_string()).collect();
                        Ok(VmValue::Str(parts.join("/")))
                    }
                    _ => Err(self.err(&format!("unknown os function '{}'", fname))),
                }
            }
            // ── time module ──────────────────────────────────────────────────
            _ if name.starts_with("time.") => {
                let fname = &name[5..];
                match fname {
                    "time" => {
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let t = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs_f64();
                        Ok(VmValue::Float(t))
                    }
                    "monotonic" => {
                        // Use a fixed epoch based on program start isn't available; use system time
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let t = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs_f64();
                        Ok(VmValue::Float(t))
                    }
                    "sleep" => {
                        let secs = match args.first() {
                            Some(VmValue::Int(i)) => *i as f64,
                            Some(VmValue::Float(f)) => *f,
                            _ => return Err(self.err("time.sleep requires a number")),
                        }
                        .max(0.0);
                        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                        Ok(VmValue::Nil)
                    }
                    _ => Err(self.err(&format!("unknown time function '{}'", fname))),
                }
            }
            // ── random module ────────────────────────────────────────────────
            _ if name.starts_with("random.") => {
                let fname = &name[7..];
                match fname {
                    "random" => Ok(VmValue::Float(self.rng_next_f64())),
                    "seed" => {
                        let seed = match args.first() {
                            Some(VmValue::Int(i)) => *i as u64,
                            Some(VmValue::Float(f)) => *f as u64,
                            _ => return Err(self.err("random.seed requires a number")),
                        };
                        self.rng = if seed == 0 { 1 } else { seed };
                        Ok(VmValue::Nil)
                    }
                    "randint" => {
                        let a = match args.first() {
                            Some(VmValue::Int(i)) => *i,
                            _ => return Err(self.err("random.randint requires ints")),
                        };
                        let b = match args.get(1) {
                            Some(VmValue::Int(i)) => *i,
                            _ => return Err(self.err("random.randint requires two ints")),
                        };
                        if a > b {
                            return Err(self.err("random.randint a must be <= b"));
                        }
                        let range = (b - a + 1) as u64;
                        Ok(VmValue::Int(a + (self.rng_next_u64() % range) as i64))
                    }
                    "choice" => {
                        let lst = match args.first() {
                            Some(VmValue::List(v)) => v.borrow().clone(),
                            Some(VmValue::Tuple(t)) => t.as_ref().clone(),
                            _ => return Err(self.err("random.choice requires a sequence")),
                        };
                        if lst.is_empty() {
                            return Err(self.err("random.choice: empty sequence"));
                        }
                        let idx = (self.rng_next_u64() as usize) % lst.len();
                        Ok(lst[idx].clone())
                    }
                    "shuffle" => {
                        let lst = match args.first() {
                            Some(VmValue::List(v)) => v.clone(),
                            _ => return Err(self.err("random.shuffle requires a list")),
                        };
                        let mut v = lst.borrow_mut();
                        for i in (1..v.len()).rev() {
                            let j = (self.rng_next_u64() as usize) % (i + 1);
                            v.swap(i, j);
                        }
                        Ok(VmValue::Nil)
                    }
                    "uniform" => {
                        let a = match args.first() {
                            Some(VmValue::Int(i)) => *i as f64,
                            Some(VmValue::Float(f)) => *f,
                            _ => return Err(self.err("random.uniform requires numbers")),
                        };
                        let b = match args.get(1) {
                            Some(VmValue::Int(i)) => *i as f64,
                            Some(VmValue::Float(f)) => *f,
                            _ => return Err(self.err("random.uniform requires two numbers")),
                        };
                        Ok(VmValue::Float(a + self.rng_next_f64() * (b - a)))
                    }
                    _ => Err(self.err(&format!("unknown random function '{}'", fname))),
                }
            }
            // ── json module ──────────────────────────────────────────────────
            _ if name.starts_with("json.") => {
                let fname = &name[5..];
                match fname {
                    "loads" => {
                        let s = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("json.loads requires a string")),
                        };
                        self.json_loads(&s)
                    }
                    "dumps" => {
                        let v = match args.first() {
                            Some(v) => v.clone(),
                            _ => return Err(self.err("json.dumps requires a value")),
                        };
                        Ok(VmValue::Str(self.json_dumps(&v)))
                    }
                    _ => Err(self.err(&format!("unknown json function '{}'", fname))),
                }
            }
            // ── toml module ──────────────────────────────────────────────────
            _ if name.starts_with("toml.") => {
                let fname = &name[5..];
                match fname {
                    "loads" => {
                        let s = match args.first() {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("toml.loads requires a string")),
                        };
                        Ok(Self::toml_data_to_vm_value(
                            &toml_runtime::loads(&s).map_err(|e| self.err(&e))?,
                        ))
                    }
                    "dumps" => {
                        let v = match args.first() {
                            Some(v) => v.clone(),
                            _ => return Err(self.err("toml.dumps requires a value")),
                        };
                        Ok(VmValue::Str(
                            toml_runtime::dumps(&self.vm_value_to_toml_data(&v)?).map_err(|e| self.err(&e))?,
                        ))
                    }
                    _ => Err(self.err(&format!("unknown toml function '{}'", fname))),
                }
            }
            // ── re module ────────────────────────────────────────────────────
            _ if name.starts_with("re.") => {
                let fname = &name[3..];
                let pattern = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err(&format!("re.{}() first arg must be string", fname))),
                };
                let text = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err(&format!("re.{}() second arg must be string", fname))),
                };
                use regex::Regex;
                let re = Regex::new(&pattern).map_err(|e| self.err(&e.to_string()))?;
                match fname {
                    "match" => Ok(VmValue::Bool(re.find(&text).map(|m| m.start() == 0).unwrap_or(false))),
                    "search" => Ok(VmValue::Bool(re.is_match(&text))),
                    "fullmatch" => Ok(VmValue::Bool(
                        re.find(&text)
                            .map(|m| m.start() == 0 && m.end() == text.len())
                            .unwrap_or(false),
                    )),
                    "findall" => {
                        let results: Vec<VmValue> = re
                            .find_iter(&text)
                            .map(|m| VmValue::Str(m.as_str().to_string()))
                            .collect();
                        Ok(VmValue::List(Rc::new(RefCell::new(results))))
                    }
                    "sub" => {
                        let repl = match args.get(2) {
                            Some(VmValue::Str(s)) => s.clone(),
                            _ => return Err(self.err("re.sub requires 3 args")),
                        };
                        Ok(VmValue::Str(re.replace_all(&text, repl.as_str()).to_string()))
                    }
                    "split" => {
                        let parts: Vec<VmValue> = re.split(&text).map(|s| VmValue::Str(s.to_string())).collect();
                        Ok(VmValue::List(Rc::new(RefCell::new(parts))))
                    }
                    _ => Err(self.err(&format!("unknown re function '{}'", fname))),
                }
            }
            _ => Err(self.err(&format!("unknown builtin '{}'", name))),
        }
    }

    // ── List method dispatch ──────────────────────────────────────────────────

    fn call_list_method(
        &mut self,
        method: &str,
        args: &[VmValue],
        _kwargs: &[(String, VmValue)],
    ) -> Result<VmValue, String> {
        // args[0] is the list (self).
        let list = match args.first() {
            Some(VmValue::List(v)) => v.clone(),
            _ => return Err(self.err(&format!("list.{}() requires a list", method))),
        };
        match method {
            "append" => {
                let item = args.get(1).cloned().unwrap_or(VmValue::Nil);
                list.borrow_mut().push(item);
                Ok(VmValue::Nil)
            }
            "pop" => {
                let idx = match args.get(1) {
                    Some(VmValue::Int(i)) => {
                        let len = list.borrow().len() as i64;
                        let i = if *i < 0 { len + i } else { *i };
                        Some(i as usize)
                    }
                    None => None,
                    _ => return Err(self.err("list.pop() index must be an int")),
                };
                let mut v = list.borrow_mut();
                if v.is_empty() {
                    return Err(self.err("pop from empty list"));
                }
                let remove_idx = idx.unwrap_or(v.len() - 1);
                Ok(v.remove(remove_idx))
            }
            "sort" => {
                list.borrow_mut().sort_by(|a, b| vm_cmp_order(a, b));
                Ok(VmValue::Nil)
            }
            "reverse" => {
                list.borrow_mut().reverse();
                Ok(VmValue::Nil)
            }
            "extend" => {
                let items = self.to_iter_vec(args.get(1).cloned().unwrap_or(VmValue::Nil))?;
                list.borrow_mut().extend(items);
                Ok(VmValue::Nil)
            }
            "insert" => {
                let idx = match args.get(1) {
                    Some(VmValue::Int(i)) => *i as usize,
                    _ => return Err(self.err("insert() idx must be int")),
                };
                let val = args.get(2).cloned().unwrap_or(VmValue::Nil);
                let mut v = list.borrow_mut();
                let idx = idx.min(v.len());
                v.insert(idx, val);
                Ok(VmValue::Nil)
            }
            "remove" => {
                let target = args.get(1).ok_or_else(|| self.err("remove() requires a value"))?;
                let mut v = list.borrow_mut();
                let pos = v
                    .iter()
                    .position(|x| vm_eq(x, target))
                    .ok_or_else(|| self.err("value not in list"))?;
                v.remove(pos);
                Ok(VmValue::Nil)
            }
            "index" => {
                let target = args.get(1).ok_or_else(|| self.err("index() requires a value"))?;
                let v = list.borrow();
                let pos = v
                    .iter()
                    .position(|x| vm_eq(x, target))
                    .ok_or_else(|| self.err("value not in list"))?;
                Ok(VmValue::Int(pos as i64))
            }
            "count" => {
                let target = args.get(1).ok_or_else(|| self.err("count() requires a value"))?;
                let v = list.borrow();
                Ok(VmValue::Int(v.iter().filter(|x| vm_eq(x, target)).count() as i64))
            }
            "clear" => {
                list.borrow_mut().clear();
                Ok(VmValue::Nil)
            }
            "copy" => {
                let v = list.borrow().clone();
                Ok(VmValue::List(Rc::new(RefCell::new(v))))
            }
            _ => Err(self.err(&format!("list has no method '{}'", method))),
        }
    }

    fn call_list_module(
        &mut self,
        method: &str,
        args: &[VmValue],
        _kwargs: &[(String, VmValue)],
    ) -> Result<VmValue, String> {
        match method {
            "sort" => {
                let list = match args.first() {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("list.sort() requires a list")),
                };
                let mut out = list;
                out.sort_by(vm_cmp_order);
                Ok(VmValue::List(Rc::new(RefCell::new(out))))
            }
            "reverse" => {
                let list = match args.first() {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("list.reverse() requires a list")),
                };
                let mut out = list;
                out.reverse();
                Ok(VmValue::List(Rc::new(RefCell::new(out))))
            }
            "map" => {
                if args.len() < 2 {
                    return Err(self.err("list.map() requires (fn, list)"));
                }
                let func = args[0].clone();
                let list = match args.get(1) {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("list.map() 2nd argument must be a list")),
                };
                let mut out = Vec::with_capacity(list.len());
                for item in list {
                    out.push(self.call_value_direct(func.clone(), &[item], &[])?);
                }
                Ok(VmValue::List(Rc::new(RefCell::new(out))))
            }
            "filter" => {
                if args.len() < 2 {
                    return Err(self.err("list.filter() requires (fn, list)"));
                }
                let func = args[0].clone();
                let list = match args.get(1) {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("list.filter() 2nd argument must be a list")),
                };
                let mut out = Vec::new();
                for item in list {
                    if self.call_value_direct(func.clone(), &[item.clone()], &[])?.is_truthy() {
                        out.push(item);
                    }
                }
                Ok(VmValue::List(Rc::new(RefCell::new(out))))
            }
            "reduce" => {
                if args.len() < 2 {
                    return Err(self.err("list.reduce() requires (fn, list[, initial])"));
                }
                let func = args[0].clone();
                let list = match args.get(1) {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("list.reduce() 2nd argument must be a list")),
                };
                let mut iter = list.into_iter();
                let mut acc = if let Some(initial) = args.get(2) {
                    initial.clone()
                } else {
                    iter.next()
                        .ok_or_else(|| self.err("list.reduce() called on empty list with no initial value"))?
                };
                for item in iter {
                    acc = self.call_value_direct(func.clone(), &[acc, item], &[])?;
                }
                Ok(acc)
            }
            "flatten" => {
                let list = match args.first() {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("list.flatten() requires a list")),
                };
                let mut out = Vec::new();
                for item in list {
                    match item {
                        VmValue::List(inner) => out.extend(inner.borrow().clone()),
                        other => out.push(other),
                    }
                }
                Ok(VmValue::List(Rc::new(RefCell::new(out))))
            }
            "unique" => {
                let list = match args.first() {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("list.unique() requires a list")),
                };
                let mut out = Vec::new();
                for item in list {
                    if !out.iter().any(|existing| vm_eq(existing, &item)) {
                        out.push(item);
                    }
                }
                Ok(VmValue::List(Rc::new(RefCell::new(out))))
            }
            _ => Err(self.err(&format!("list module has no function '{}'", method))),
        }
    }

    fn call_string_module(
        &mut self,
        method: &str,
        args: &[VmValue],
        _kwargs: &[(String, VmValue)],
    ) -> Result<VmValue, String> {
        let req_str = |idx: usize, name: &str, args: &[VmValue]| -> Result<String, String> {
            match args.get(idx) {
                Some(VmValue::Str(s)) => Ok(s.clone()),
                _ => Err(self.err(&format!("string.{}() requires string argument {}", name, idx + 1))),
            }
        };
        match method {
            "split" => {
                let s = req_str(0, method, args)?;
                let parts: Vec<VmValue> = match args.get(1) {
                    Some(VmValue::Str(sep)) => s.split(sep).map(|p| VmValue::Str(p.to_string())).collect(),
                    None => s.split_whitespace().map(|p| VmValue::Str(p.to_string())).collect(),
                    _ => return Err(self.err("string.split() separator must be a string")),
                };
                Ok(VmValue::List(Rc::new(RefCell::new(parts))))
            }
            "join" => {
                let sep = req_str(0, method, args)?;
                let list = match args.get(1) {
                    Some(VmValue::List(v)) => v.borrow().clone(),
                    _ => return Err(self.err("string.join() requires a list as 2nd argument")),
                };
                Ok(VmValue::Str(
                    list.into_iter().map(|v| v.to_string()).collect::<Vec<_>>().join(&sep),
                ))
            }
            "strip" => Ok(VmValue::Str(req_str(0, method, args)?.trim().to_string())),
            "lstrip" => Ok(VmValue::Str(req_str(0, method, args)?.trim_start().to_string())),
            "rstrip" => Ok(VmValue::Str(req_str(0, method, args)?.trim_end().to_string())),
            "upper" => Ok(VmValue::Str(req_str(0, method, args)?.to_uppercase())),
            "lower" => Ok(VmValue::Str(req_str(0, method, args)?.to_lowercase())),
            "title" => Ok(VmValue::Str(
                req_str(0, method, args)?
                    .split_whitespace()
                    .map(|w| {
                        let mut c = w.chars();
                        c.next()
                            .map(|f| f.to_uppercase().to_string() + c.as_str())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            )),
            "capitalize" => {
                let s = req_str(0, method, args)?;
                let mut c = s.chars();
                Ok(VmValue::Str(
                    c.next()
                        .map(|f| f.to_uppercase().to_string() + c.as_str())
                        .unwrap_or_default(),
                ))
            }
            "replace" => Ok(VmValue::Str(
                req_str(0, method, args)?.replace(&req_str(1, method, args)?, &req_str(2, method, args)?),
            )),
            "startswith" => Ok(VmValue::Bool(
                req_str(0, method, args)?.starts_with(&req_str(1, method, args)?),
            )),
            "endswith" => Ok(VmValue::Bool(
                req_str(0, method, args)?.ends_with(&req_str(1, method, args)?),
            )),
            "find" => Ok(VmValue::Int(
                req_str(0, method, args)?
                    .find(&req_str(1, method, args)?)
                    .map(|i| i as i64)
                    .unwrap_or(-1),
            )),
            "count" => Ok(VmValue::Int(
                req_str(0, method, args)?.matches(&req_str(1, method, args)?).count() as i64,
            )),
            "format" => {
                let mut result = req_str(0, method, args)?;
                for arg in args.iter().skip(1) {
                    if let Some(pos) = result.find("{}") {
                        result.replace_range(pos..pos + 2, &arg.to_string());
                    }
                }
                Ok(VmValue::Str(result))
            }
            _ => Err(self.err(&format!("string has no function '{}'", method))),
        }
    }

    // ── Dict method dispatch ──────────────────────────────────────────────────

    fn call_dict_method(
        &mut self,
        method: &str,
        args: &[VmValue],
        _kwargs: &[(String, VmValue)],
    ) -> Result<VmValue, String> {
        let dict = match args.first() {
            Some(VmValue::Dict(d)) => d.clone(),
            _ => return Err(self.err(&format!("dict.{}() requires a dict", method))),
        };
        match method {
            "keys" => Ok(VmValue::List(Rc::new(RefCell::new(dict.borrow().keys.clone())))),
            "values" => Ok(VmValue::List(Rc::new(RefCell::new(dict.borrow().vals.clone())))),
            "items" => {
                let d = dict.borrow();
                let items: Vec<VmValue> = d
                    .keys
                    .iter()
                    .zip(d.vals.iter())
                    .map(|(k, v)| VmValue::Tuple(Rc::new(vec![k.clone(), v.clone()])))
                    .collect();
                Ok(VmValue::List(Rc::new(RefCell::new(items))))
            }
            "get" => {
                let key = args.get(1).ok_or_else(|| self.err("dict.get() requires a key"))?;
                let default = args.get(2).cloned().unwrap_or(VmValue::Nil);
                Ok(dict.borrow().get(key).unwrap_or(default))
            }
            "pop" => {
                let key = args.get(1).ok_or_else(|| self.err("dict.pop() requires a key"))?;
                let default = args.get(2).cloned();
                match dict.borrow().get(key) {
                    Some(v) => {
                        dict.borrow_mut().remove(key);
                        Ok(v)
                    }
                    None => default.ok_or_else(|| self.err("key not found")),
                }
            }
            "update" => {
                if let Some(VmValue::Dict(other)) = args.get(1) {
                    let pairs: Vec<_> = other
                        .borrow()
                        .keys
                        .iter()
                        .zip(other.borrow().vals.iter())
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (k, v) in pairs {
                        dict.borrow_mut().set(k, v);
                    }
                }
                Ok(VmValue::Nil)
            }
            "clear" => {
                dict.borrow_mut().keys.clear();
                dict.borrow_mut().vals.clear();
                Ok(VmValue::Nil)
            }
            "copy" => Ok(VmValue::Dict(Rc::new(RefCell::new(dict.borrow().clone())))),
            "contains" | "has_key" => {
                let key = args.get(1).ok_or_else(|| self.err("dict.contains() requires a key"))?;
                Ok(VmValue::Bool(dict.borrow().get(key).is_some()))
            }
            _ => Err(self.err(&format!("dict has no method '{}'", method))),
        }
    }

    // ── String method dispatch ────────────────────────────────────────────────

    fn call_str_method(&self, spec: &str, args: &[VmValue], _kwargs: &[(String, VmValue)]) -> Result<VmValue, String> {
        // spec is "method_name:self_str"
        let colon = spec.find(':').ok_or_else(|| self.err("invalid str method spec"))?;
        let method = &spec[..colon];
        let s = spec[colon + 1..].to_string();

        match method {
            "upper" => Ok(VmValue::Str(s.to_uppercase())),
            "lower" => Ok(VmValue::Str(s.to_lowercase())),
            "strip" => Ok(VmValue::Str(s.trim().to_string())),
            "lstrip" => Ok(VmValue::Str(s.trim_start().to_string())),
            "rstrip" => Ok(VmValue::Str(s.trim_end().to_string())),
            "title" => {
                let t = s
                    .split_whitespace()
                    .map(|w| {
                        let mut c = w.chars();
                        c.next()
                            .map(|f| f.to_uppercase().to_string() + c.as_str())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                Ok(VmValue::Str(t))
            }
            "capitalize" => {
                let mut c = s.chars();
                let cap = c
                    .next()
                    .map(|f| f.to_uppercase().to_string() + c.as_str())
                    .unwrap_or_default();
                Ok(VmValue::Str(cap))
            }
            "split" => {
                // args[0] = receiver (string), args[1] = optional separator
                let sep = match args.get(1) {
                    Some(VmValue::Str(s)) => Some(s.as_str().to_string()),
                    _ => None,
                };
                let parts: Vec<VmValue> = match sep {
                    Some(sep) => s.split(sep.as_str()).map(|p| VmValue::Str(p.to_string())).collect(),
                    None => s.split_whitespace().map(|p| VmValue::Str(p.to_string())).collect(),
                };
                Ok(VmValue::List(Rc::new(RefCell::new(parts))))
            }
            "replace" => {
                let from = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("replace() requires 2 string args")),
                };
                let to = match args.get(2) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("replace() requires 2 string args")),
                };
                Ok(VmValue::Str(s.replace(from.as_str(), &to)))
            }
            "find" => {
                let sub = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("find() requires a string")),
                };
                Ok(VmValue::Int(s.find(sub.as_str()).map(|i| i as i64).unwrap_or(-1)))
            }
            "count" => {
                let sub = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("count() requires a string")),
                };
                Ok(VmValue::Int(s.matches(sub.as_str()).count() as i64))
            }
            "startswith" => {
                let pre = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("startswith() requires a string")),
                };
                Ok(VmValue::Bool(s.starts_with(pre.as_str())))
            }
            "endswith" => {
                let suf = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("endswith() requires a string")),
                };
                Ok(VmValue::Bool(s.ends_with(suf.as_str())))
            }
            "join" => {
                // args[0] = receiver (separator string), args[1] = iterable
                let items = self.to_iter_vec(args.get(1).cloned().unwrap_or(VmValue::Nil))?;
                let parts: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                Ok(VmValue::Str(parts.join(&s)))
            }
            "format" => {
                let mut result = s.clone();
                // args[0] = receiver; user args start at 1
                for arg in args.iter().skip(1) {
                    if let Some(pos) = result.find("{}") {
                        result = format!("{}{}{}", &result[..pos], arg, &result[pos + 2..]);
                    }
                }
                Ok(VmValue::Str(result))
            }
            "isdigit" => Ok(VmValue::Bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))),
            "isalpha" => Ok(VmValue::Bool(!s.is_empty() && s.chars().all(|c| c.is_alphabetic()))),
            "isspace" => Ok(VmValue::Bool(!s.is_empty() && s.chars().all(|c| c.is_whitespace()))),
            "zfill" => {
                let width = match args.get(1) {
                    Some(VmValue::Int(n)) => *n as usize,
                    _ => return Err(self.err("zfill() requires an int")),
                };
                Ok(VmValue::Str(format!("{:0>width$}", s)))
            }
            "encode" => Ok(VmValue::Str(s)), // stub
            _ => Err(self.err(&format!("str has no method '{}'", method))),
        }
    }

    // ── File method dispatch ──────────────────────────────────────────────────

    fn call_file_method(
        &self,
        method: &str,
        args: &[VmValue],
        _kwargs: &[(String, VmValue)],
    ) -> Result<VmValue, String> {
        let fh_rc = match args.first() {
            Some(VmValue::File(f)) => f.clone(),
            _ => return Err(self.err(&format!("file.{}() requires a file", method))),
        };
        match method {
            "read" => {
                let (mode, path) = {
                    let fh = fh_rc.borrow();
                    (fh.mode.clone(), fh.path.clone())
                };
                if mode.contains('r') {
                    let full_path = self.source_dir.join(&path);
                    let content = std::fs::read_to_string(&full_path)
                        .map_err(|e| self.err(&format!("read '{}': {}", path, e)))?;
                    Ok(VmValue::Str(content))
                } else {
                    Err(self.err("file not open for reading"))
                }
            }
            "readline" => {
                let mut fh = fh_rc.borrow_mut();
                if fh.line_pos < fh.content.len() {
                    let line = fh.content[fh.line_pos].clone() + "\n";
                    fh.line_pos += 1;
                    Ok(VmValue::Str(line))
                } else {
                    Ok(VmValue::Str(String::new()))
                }
            }
            "readlines" => {
                let (mode, path) = {
                    let fh = fh_rc.borrow();
                    (fh.mode.clone(), fh.path.clone())
                };
                if mode.contains('r') {
                    let full_path = self.source_dir.join(&path);
                    let content = std::fs::read_to_string(&full_path)
                        .map_err(|e| self.err(&format!("readlines '{}': {}", path, e)))?;
                    let lines: Vec<VmValue> = content.lines().map(|l| VmValue::Str(l.to_string() + "\n")).collect();
                    Ok(VmValue::List(Rc::new(RefCell::new(lines))))
                } else {
                    Err(self.err("file not open for reading"))
                }
            }
            "write" => {
                let text = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("write() requires a string")),
                };
                let mut fh = fh_rc.borrow_mut();
                fh.write_buf.push_str(&text);
                let full_path = self.source_dir.join(&fh.path);
                let existing = if fh.mode == "a" || fh.mode == "a+" {
                    std::fs::read_to_string(&full_path).unwrap_or_default()
                } else {
                    String::new()
                };
                std::fs::write(&full_path, existing + &fh.write_buf).map_err(|e| self.err(&format!("write: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "close" => {
                fh_rc.borrow_mut().closed = true;
                Ok(VmValue::Nil)
            }
            _ => Err(self.err(&format!("file has no method '{}'", method))),
        }
    }

    fn vm_socket_method(&self, receiver: VmValue, name: &str) -> Result<VmValue, String> {
        match name {
            "send" | "recv" | "readline" | "accept" | "close" => {
                Ok(VmValue::BoundBuiltin(Box::new(receiver), format!("socket.{}", name)))
            }
            "__enter__" => Ok(VmValue::BoundBuiltin(
                Box::new(receiver),
                "socket.__enter__".to_string(),
            )),
            "__exit__" => Ok(VmValue::BoundBuiltin(Box::new(receiver), "socket.__exit__".to_string())),
            _ => Err(self.err(&format!("socket has no method '{}'", name))),
        }
    }

    fn call_socket_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        use crate::opcode::{VmSocket, VmSocketKind};
        match name {
            "connect" => {
                let host = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("socket.connect() requires a host string")),
                };
                let port = match args.get(1) {
                    Some(VmValue::Int(p)) => *p as u16,
                    _ => return Err(self.err("socket.connect() requires an integer port")),
                };
                let addr = format!("{host}:{port}");
                let stream = std::net::TcpStream::connect(&addr)
                    .map_err(|e| self.err(&format!("socket.connect() error: {e}")))?;
                Ok(VmValue::Socket(Rc::new(RefCell::new(VmSocket {
                    kind: VmSocketKind::Stream(stream),
                    closed: false,
                    peer: addr,
                }))))
            }
            "listen" => {
                let host = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("socket.listen() requires a host string")),
                };
                let port = match args.get(1) {
                    Some(VmValue::Int(p)) => *p as u16,
                    _ => return Err(self.err("socket.listen() requires an integer port")),
                };
                let addr = format!("{host}:{port}");
                let listener =
                    std::net::TcpListener::bind(&addr).map_err(|e| self.err(&format!("socket.listen() error: {e}")))?;
                Ok(VmValue::Socket(Rc::new(RefCell::new(VmSocket {
                    kind: VmSocketKind::Listener(listener),
                    closed: false,
                    peer: addr,
                }))))
            }
            "__enter__" => Ok(args.first().cloned().unwrap_or(VmValue::Nil)),
            "__exit__" => {
                if let Some(VmValue::Socket(sh)) = args.first() {
                    sh.borrow_mut().closed = true;
                }
                Ok(VmValue::Nil)
            }
            "send" => {
                let sh_rc = match args.first() {
                    Some(VmValue::Socket(s)) => s.clone(),
                    _ => return Err(self.err("socket.send() requires a socket")),
                };
                let data = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    Some(v) => v.to_string(),
                    None => return Err(self.err("send() requires 1 argument")),
                };
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("send() on closed socket"));
                }
                match &mut sh.kind {
                    VmSocketKind::Stream(stream) => {
                        use std::io::Write as IoWrite;
                        let bytes = data.as_bytes();
                        stream
                            .write_all(bytes)
                            .map_err(|e| self.err(&format!("socket.send() error: {e}")))?;
                        Ok(VmValue::Int(bytes.len() as i64))
                    }
                    VmSocketKind::Listener(_) => Err(self.err("send() on server socket")),
                }
            }
            "recv" => {
                let sh_rc = match args.first() {
                    Some(VmValue::Socket(s)) => s.clone(),
                    _ => return Err(self.err("socket.recv() requires a socket")),
                };
                let size = match args.get(1) {
                    Some(VmValue::Int(n)) => *n as usize,
                    Some(_) => return Err(self.err("recv() requires an integer size")),
                    None => 4096,
                };
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("recv() on closed socket"));
                }
                match &mut sh.kind {
                    VmSocketKind::Stream(stream) => {
                        use std::io::Read;
                        let mut buf = vec![0u8; size];
                        let n = stream
                            .read(&mut buf)
                            .map_err(|e| self.err(&format!("socket.recv() error: {e}")))?;
                        Ok(VmValue::Str(String::from_utf8_lossy(&buf[..n]).to_string()))
                    }
                    VmSocketKind::Listener(_) => Err(self.err("recv() on server socket")),
                }
            }
            "readline" => {
                let sh_rc = match args.first() {
                    Some(VmValue::Socket(s)) => s.clone(),
                    _ => return Err(self.err("socket.readline() requires a socket")),
                };
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("readline() on closed socket"));
                }
                match &mut sh.kind {
                    VmSocketKind::Stream(stream) => {
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
                        Ok(VmValue::Str(line))
                    }
                    VmSocketKind::Listener(_) => Err(self.err("readline() on server socket")),
                }
            }
            "accept" => {
                let sh_rc = match args.first() {
                    Some(VmValue::Socket(s)) => s.clone(),
                    _ => return Err(self.err("socket.accept() requires a socket")),
                };
                let mut sh = sh_rc.borrow_mut();
                if sh.closed {
                    return Err(self.err("accept() on closed socket"));
                }
                match &mut sh.kind {
                    VmSocketKind::Listener(listener) => {
                        let (stream, addr) = listener
                            .accept()
                            .map_err(|e| self.err(&format!("socket.accept() error: {e}")))?;
                        let peer = addr.to_string();
                        Ok(VmValue::Socket(Rc::new(RefCell::new(VmSocket {
                            kind: VmSocketKind::Stream(stream),
                            closed: false,
                            peer,
                        }))))
                    }
                    VmSocketKind::Stream(_) => Err(self.err("accept() on client socket")),
                }
            }
            "close" => {
                if let Some(VmValue::Socket(sh)) = args.first() {
                    sh.borrow_mut().closed = true;
                }
                Ok(VmValue::Nil)
            }
            other => Err(self.err(&format!("socket has no function '{other}'"))),
        }
    }

    fn subprocess_result_value(&self, result: SubprocessResult) -> VmValue {
        let mut d = VmDict::new();
        d.set(
            VmValue::Str("code".to_string()),
            result.code.map(VmValue::Int).unwrap_or(VmValue::Nil),
        );
        d.set(VmValue::Str("stdout".to_string()), VmValue::Str(result.stdout));
        d.set(VmValue::Str("stderr".to_string()), VmValue::Str(result.stderr));
        d.set(VmValue::Str("timed_out".to_string()), VmValue::Bool(result.timed_out));
        d.set(
            VmValue::Str("ok".to_string()),
            VmValue::Bool(!result.timed_out && result.code == Some(0)),
        );
        VmValue::Dict(Rc::new(RefCell::new(d)))
    }

    fn subprocess_timeout_arg(&self, args: &[VmValue], idx: usize, name: &str) -> Result<Option<f64>, String> {
        match args.get(idx) {
            None | Some(VmValue::Nil) => Ok(None),
            Some(VmValue::Int(i)) => Ok(Some(*i as f64)),
            Some(VmValue::Float(f)) => Ok(Some(*f)),
            _ => Err(self.err(&format!("subprocess.{} timeout must be a number", name))),
        }
    }

    fn call_subprocess_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        if args.is_empty() || args.len() > 2 {
            return Err(self.err(&format!("subprocess.{} takes 1 or 2 arguments", name)));
        }

        let command = match args.first() {
            Some(VmValue::Str(s)) => s.clone(),
            _ => return Err(self.err(&format!("subprocess.{} requires a command string", name))),
        };
        let timeout = self.subprocess_timeout_arg(args, 1, name)?;
        let result =
            run_shell_command(&command, timeout).map_err(|e| self.err(&format!("subprocess.{} error: {}", name, e)))?;

        match name {
            "run" => Ok(self.subprocess_result_value(result)),
            "call" => Ok(result.code.map(VmValue::Int).unwrap_or(VmValue::Nil)),
            "check_output" => {
                if result.timed_out {
                    return Err(self.err("subprocess.check_output timed out"));
                }
                if result.code != Some(0) {
                    let code = result.code.map(|n| n.to_string()).unwrap_or_else(|| "nil".to_string());
                    let detail = if result.stderr.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", result.stderr.trim_end())
                    };
                    return Err(self.err(&format!("subprocess.check_output exited with code {}{}", code, detail)));
                }
                Ok(VmValue::Str(result.stdout))
            }
            _ => Err(self.err(&format!("unknown subprocess function '{}'", name))),
        }
    }

    fn vm_value_to_arg_data(&self, value: &VmValue) -> Result<ArgData, String> {
        match value {
            VmValue::Int(n) => Ok(ArgData::Int(*n)),
            VmValue::Float(f) => Ok(ArgData::Float(*f)),
            VmValue::Str(s) => Ok(ArgData::Str(s.clone())),
            VmValue::Bool(b) => Ok(ArgData::Bool(*b)),
            VmValue::Nil => Ok(ArgData::Nil),
            VmValue::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.vm_value_to_arg_data(item)?);
                }
                Ok(ArgData::List(out))
            }
            VmValue::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.vm_value_to_arg_data(item)?);
                }
                Ok(ArgData::Tuple(out))
            }
            VmValue::Dict(dict) => {
                let dict = dict.borrow();
                let mut out = Vec::with_capacity(dict.keys.len());
                for (key, value) in dict.keys.iter().zip(dict.vals.iter()) {
                    out.push((self.vm_value_to_arg_data(key)?, self.vm_value_to_arg_data(value)?));
                }
                Ok(ArgData::Dict(out))
            }
            other => Err(self.err(&format!(
                "argparse only accepts scalar/list/tuple/dict values, got {}",
                other.type_name()
            ))),
        }
    }

    fn arg_data_to_vm_value(data: &ArgData) -> VmValue {
        match data {
            ArgData::Int(n) => VmValue::Int(*n),
            ArgData::Float(f) => VmValue::Float(*f),
            ArgData::Str(s) => VmValue::Str(s.clone()),
            ArgData::Bool(b) => VmValue::Bool(*b),
            ArgData::Nil => VmValue::Nil,
            ArgData::List(items) => VmValue::List(Rc::new(RefCell::new(
                items.iter().map(Self::arg_data_to_vm_value).collect(),
            ))),
            ArgData::Tuple(items) => VmValue::Tuple(Rc::new(items.iter().map(Self::arg_data_to_vm_value).collect())),
            ArgData::Dict(items) => {
                let mut out = VmDict::new();
                for (key, value) in items {
                    out.set(Self::arg_data_to_vm_value(key), Self::arg_data_to_vm_value(value));
                }
                VmValue::Dict(Rc::new(RefCell::new(out)))
            }
        }
    }

    fn argparse_argv_arg(&self, value: Option<&VmValue>) -> Result<Vec<String>, String> {
        match value {
            None | Some(VmValue::Nil) => Ok(argparse_runtime::current_process_argv().into_iter().skip(1).collect()),
            Some(VmValue::List(items)) => items
                .borrow()
                .iter()
                .map(|item| match item {
                    VmValue::Str(s) => Ok(s.clone()),
                    other => Err(self.err(&format!(
                        "argparse.parse() argv items must be strings, got {}",
                        other.type_name()
                    ))),
                })
                .collect(),
            Some(VmValue::Tuple(items)) => items
                .iter()
                .map(|item| match item {
                    VmValue::Str(s) => Ok(s.clone()),
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

    fn call_argparse_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "parse" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("argparse.parse() takes a spec dict and optional argv list"));
                }
                let spec = self.vm_value_to_arg_data(&args[0])?;
                let argv = self.argparse_argv_arg(args.get(1))?;
                let parsed = argparse_runtime::parse(&spec, &argv, Some(&argparse_runtime::default_prog_name()))
                    .map_err(|e| self.err(&e))?;
                Ok(Self::arg_data_to_vm_value(&parsed))
            }
            "help" => {
                if args.len() != 1 {
                    return Err(self.err("argparse.help() takes exactly one spec dict"));
                }
                let spec = self.vm_value_to_arg_data(&args[0])?;
                let rendered = argparse_runtime::help(&spec, Some(&argparse_runtime::default_prog_name()))
                    .map_err(|e| self.err(&e))?;
                Ok(VmValue::Str(rendered))
            }
            _ => Err(self.err(&format!("unknown argparse function '{}'", name))),
        }
    }

    fn vm_value_to_log_data(&self, value: &VmValue) -> Result<LogData, String> {
        match value {
            VmValue::Int(n) => Ok(LogData::Int(*n)),
            VmValue::Float(f) => Ok(LogData::Float(*f)),
            VmValue::Str(s) => Ok(LogData::Str(s.clone())),
            VmValue::Bool(b) => Ok(LogData::Bool(*b)),
            VmValue::Nil => Ok(LogData::Nil),
            VmValue::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.vm_value_to_log_data(item)?);
                }
                Ok(LogData::List(out))
            }
            VmValue::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.vm_value_to_log_data(item)?);
                }
                Ok(LogData::Tuple(out))
            }
            VmValue::Dict(map) => {
                let map = map.borrow();
                let mut out = Vec::with_capacity(map.keys.len());
                for (key, value) in map.keys.iter().zip(map.vals.iter()) {
                    out.push((self.vm_value_to_log_data(key)?, self.vm_value_to_log_data(value)?));
                }
                Ok(LogData::Dict(out))
            }
            other => Err(self.err(&format!(
                "logging only accepts scalar/list/tuple/dict values, got {}",
                other.type_name()
            ))),
        }
    }

    fn call_logging_module(&mut self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "basic_config" => {
                if args.len() > 1 {
                    return Err(self.err("logging.basic_config() takes at most one config dict"));
                }
                let config = match args.first() {
                    None => None,
                    Some(value) => Some(self.vm_value_to_log_data(value)?),
                };
                logging_runtime::configure(&mut self.logging_state, config.as_ref()).map_err(|e| self.err(&e))?;
                Ok(VmValue::Nil)
            }
            "log" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(self.err("logging.log() takes a level string, message, and optional logger name"));
                }
                let level = match &args[0] {
                    VmValue::Str(s) => LogLevel::parse(s).map_err(|e| self.err(&e))?,
                    other => {
                        return Err(self.err(&format!(
                            "logging.log() level must be a string, got {}",
                            other.type_name()
                        )))
                    }
                };
                let message = args[1].to_string();
                let logger_name = args.get(2).map(|value| value.to_string());
                logging_runtime::emit(&mut self.logging_state, level, &message, logger_name.as_deref())
                    .map_err(|e| self.err(&e))?;
                Ok(VmValue::Nil)
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
                let message = args[0].to_string();
                let logger_name = args.get(1).map(|value| value.to_string());
                logging_runtime::emit(&mut self.logging_state, level, &message, logger_name.as_deref())
                    .map_err(|e| self.err(&e))?;
                Ok(VmValue::Nil)
            }
            _ => Err(self.err(&format!("unknown logging function '{}'", name))),
        }
    }

    fn test_assertion_failure(&mut self, message: String) -> Result<VmValue, String> {
        let exc = VmValue::Str(format!("AssertionError: {}", message));
        let rendered = exc.to_string();
        self.current_exc = Some(exc);
        Err(format!(
            "Unhandled exception (line {}): {}",
            self.current_line, rendered
        ))
    }

    fn test_message_arg(&self, args: &[VmValue], idx: usize, fname: &str) -> Result<Option<String>, String> {
        match args.get(idx) {
            None => Ok(None),
            Some(VmValue::Nil) => Ok(None),
            Some(value) => {
                if args.len() > idx + 1 {
                    return Err(self.err(&format!("test.{}() takes at most {} arguments", fname, idx + 1)));
                }
                Ok(Some(value.to_string()))
            }
        }
    }

    fn test_default_message(&self, message: Option<String>, default: impl FnOnce() -> String) -> String {
        message.unwrap_or_else(default)
    }

    fn test_args_list(&self, value: Option<&VmValue>) -> Result<Vec<VmValue>, String> {
        match value {
            None | Some(VmValue::Nil) => Ok(Vec::new()),
            Some(VmValue::List(items)) => Ok(items.borrow().clone()),
            Some(VmValue::Tuple(items)) => Ok(items.iter().cloned().collect()),
            Some(other) => Err(self.err(&format!(
                "test.raises() args must be a list or tuple, got {}",
                other.type_name()
            ))),
        }
    }

    fn test_expected_exc_name(&self, value: Option<&VmValue>) -> Result<Option<String>, String> {
        match value {
            None | Some(VmValue::Nil) => Ok(None),
            Some(VmValue::Str(name)) => Ok(Some(name.clone())),
            Some(VmValue::Class(cls)) => Ok(Some(cls.name.clone())),
            Some(other) => Err(self.err(&format!(
                "test.raises() expected exception must be a string/class or nil, got {}",
                other.type_name()
            ))),
        }
    }

    fn test_exception_value(&self, err: &str) -> VmValue {
        match &self.current_exc {
            Some(VmValue::Nil) | None => {
                let parsed = err
                    .split_once("): ")
                    .map(|(_, tail)| tail.to_string())
                    .unwrap_or_else(|| err.to_string());
                VmValue::Str(parsed)
            }
            Some(value) => value.clone(),
        }
    }

    fn call_test_module(&mut self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "equal" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(self.err("test.equal() takes actual, expected, and optional message"));
                }
                if !vm_eq(&args[0], &args[1]) {
                    let message = self.test_default_message(self.test_message_arg(args, 2, name)?, || {
                        format!("expected {} == {}", vm_repr(&args[0]), vm_repr(&args[1]))
                    });
                    return self.test_assertion_failure(message);
                }
                Ok(VmValue::Nil)
            }
            "not_equal" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(self.err("test.not_equal() takes actual, expected, and optional message"));
                }
                if vm_eq(&args[0], &args[1]) {
                    let message = self.test_default_message(self.test_message_arg(args, 2, name)?, || {
                        format!("expected {} != {}", vm_repr(&args[0]), vm_repr(&args[1]))
                    });
                    return self.test_assertion_failure(message);
                }
                Ok(VmValue::Nil)
            }
            "true" | "truthy" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.truthy() takes a value and optional message"));
                }
                if !args[0].is_truthy() {
                    let message = self
                        .test_default_message(self.test_message_arg(args, 1, name)?, || "expected truthy value".into());
                    return self.test_assertion_failure(message);
                }
                Ok(VmValue::Nil)
            }
            "false" | "falsey" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.falsey() takes a value and optional message"));
                }
                if args[0].is_truthy() {
                    let message = self
                        .test_default_message(self.test_message_arg(args, 1, name)?, || "expected falsey value".into());
                    return self.test_assertion_failure(message);
                }
                Ok(VmValue::Nil)
            }
            "nil" | "is_nil" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.is_nil() takes a value and optional message"));
                }
                if !matches!(args[0], VmValue::Nil) {
                    let message = self.test_default_message(self.test_message_arg(args, 1, name)?, || {
                        format!("expected nil, got {}", vm_repr(&args[0]))
                    });
                    return self.test_assertion_failure(message);
                }
                Ok(VmValue::Nil)
            }
            "not_nil" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("test.not_nil() takes a value and optional message"));
                }
                if matches!(args[0], VmValue::Nil) {
                    let message = self.test_default_message(self.test_message_arg(args, 1, name)?, || {
                        "expected non-nil value".into()
                    });
                    return self.test_assertion_failure(message);
                }
                Ok(VmValue::Nil)
            }
            "fail" => {
                if args.len() > 1 {
                    return Err(self.err("test.fail() takes at most one message argument"));
                }
                let message = args
                    .first()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "test.fail() called".to_string());
                self.test_assertion_failure(message)
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
                let frame_depth = self.frames.len();
                let stack_depth = self.stack.len();
                let handler_depth = self.exc_handlers.len();
                let call_result = self.call_value_direct(callable, &call_args, &[]);
                while self.frames.len() > frame_depth {
                    let fb = self.frames.last().unwrap().base;
                    self.close_upvalues_above(fb);
                    self.stack.truncate(fb);
                    self.frames.pop();
                }
                self.exc_handlers.truncate(handler_depth);
                self.stack.truncate(stack_depth);
                match call_result {
                    Ok(_) => {
                        self.test_assertion_failure("expected exception, but call returned successfully".to_string())
                    }
                    Err(err) => {
                        if !err.starts_with("Unhandled exception (line ") {
                            return self
                                .test_assertion_failure(format!("expected exception, got runtime error: {}", err));
                        }
                        let exc = self.test_exception_value(&err);
                        if let Some(expected_name) = expected {
                            let matches = match &exc {
                                VmValue::Instance(inst) => {
                                    let exc_cls = Rc::new(VmClass {
                                        name: expected_name.clone(),
                                        parent: None,
                                        methods: HashMap::new(),
                                        class_vars: RefCell::new(HashMap::new()),
                                    });
                                    self.is_instance_of(&inst.class, &exc_cls)
                                }
                                VmValue::Str(s) => {
                                    s == &expected_name
                                        || s.starts_with(&format!("{}:", expected_name))
                                        || expected_name == "Exception"
                                }
                                other => other.type_name() == expected_name,
                            };
                            if !matches {
                                return self.test_assertion_failure(format!(
                                    "expected exception {}, got {}",
                                    expected_name, err
                                ));
                            }
                        }
                        Ok(exc)
                    }
                }
            }
            _ => Err(self.err(&format!("unknown test function '{}'", name))),
        }
    }

    fn csv_rows_to_value(&self, rows: Vec<Vec<String>>) -> VmValue {
        VmValue::List(Rc::new(RefCell::new(
            rows.into_iter()
                .map(|row| {
                    VmValue::List(Rc::new(RefCell::new(
                        row.into_iter().map(VmValue::Str).collect::<Vec<_>>(),
                    )))
                })
                .collect(),
        )))
    }

    fn csv_dicts_to_value(&self, rows: Vec<Vec<(String, String)>>) -> VmValue {
        VmValue::List(Rc::new(RefCell::new(
            rows.into_iter()
                .map(|row| {
                    let mut map = VmDict::new();
                    for (key, value) in row {
                        map.set(VmValue::Str(key), VmValue::Str(value));
                    }
                    VmValue::Dict(Rc::new(RefCell::new(map)))
                })
                .collect(),
        )))
    }

    fn csv_write_rows_arg(&self, value: &VmValue) -> Result<Vec<Vec<String>>, String> {
        let rows: Vec<VmValue> = match value {
            VmValue::List(items) => items.borrow().clone(),
            VmValue::Tuple(items) => items.iter().cloned().collect(),
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

        if matches!(first, VmValue::Dict(_)) {
            let header_keys = match first {
                VmValue::Dict(map) => map.borrow().keys.clone(),
                _ => unreachable!(),
            };
            let header_row: Vec<String> = header_keys.iter().map(|key| key.to_string()).collect();
            let mut out = Vec::with_capacity(rows.len() + 1);
            out.push(header_row);
            for row in rows {
                let map = match row {
                    VmValue::Dict(map) => map,
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
                    cols.push(map.get(key).map(|value| value.to_string()).unwrap_or_default());
                }
                out.push(cols);
            }
            Ok(out)
        } else {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                match row {
                    VmValue::List(items) => {
                        out.push(items.borrow().iter().map(|value| value.to_string()).collect());
                    }
                    VmValue::Tuple(items) => {
                        out.push(items.iter().map(|value| value.to_string()).collect());
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

    fn call_csv_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "rows" => {
                if args.len() != 1 {
                    return Err(self.err("csv.rows() takes exactly one string argument"));
                }
                let text = match &args[0] {
                    VmValue::Str(s) => s,
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
                    VmValue::Str(s) => s,
                    other => return Err(self.err(&format!("csv.dicts() requires a string, got {}", other.type_name()))),
                };
                let rows = csv_runtime::parse_dicts(text).map_err(|e| self.err(&e))?;
                Ok(self.csv_dicts_to_value(rows))
            }
            "write" => {
                if args.len() != 1 {
                    return Err(self.err("csv.write() takes exactly one rows argument"));
                }
                Ok(VmValue::Str(csv_runtime::write_rows(
                    &self.csv_write_rows_arg(&args[0])?,
                )))
            }
            _ => Err(self.err(&format!("unknown csv function '{}'", name))),
        }
    }

    fn datetime_parts_to_value(&self, parts: DateTimeParts) -> VmValue {
        let mut map = VmDict::new();
        map.set(VmValue::Str("year".to_string()), VmValue::Int(parts.year));
        map.set(VmValue::Str("month".to_string()), VmValue::Int(parts.month));
        map.set(VmValue::Str("day".to_string()), VmValue::Int(parts.day));
        map.set(VmValue::Str("hour".to_string()), VmValue::Int(parts.hour));
        map.set(VmValue::Str("minute".to_string()), VmValue::Int(parts.minute));
        map.set(VmValue::Str("second".to_string()), VmValue::Int(parts.second));
        map.set(VmValue::Str("weekday".to_string()), VmValue::Int(parts.weekday));
        map.set(VmValue::Str("yearday".to_string()), VmValue::Int(parts.yearday));
        VmValue::Dict(Rc::new(RefCell::new(map)))
    }

    fn datetime_number_arg(&self, value: &VmValue, context: &str) -> Result<f64, String> {
        match value {
            VmValue::Int(n) => Ok(*n as f64),
            VmValue::Float(f) if f.is_finite() => Ok(*f),
            VmValue::Float(_) => Err(self.err(&format!("{context} must be a finite number"))),
            other => Err(self.err(&format!("{context} must be a number, got {}", other.type_name()))),
        }
    }

    fn datetime_format_arg<'a>(&self, value: Option<&'a VmValue>, name: &str) -> Result<Option<&'a str>, String> {
        match value {
            None | Some(VmValue::Nil) => Ok(None),
            Some(VmValue::Str(s)) => Ok(Some(s.as_str())),
            Some(other) => Err(self.err(&format!(
                "datetime.{}() format must be a string or nil, got {}",
                name,
                other.type_name()
            ))),
        }
    }

    fn call_datetime_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "now" => {
                if !args.is_empty() {
                    return Err(self.err("datetime.now() takes no arguments"));
                }
                Ok(VmValue::Float(datetime_runtime::now()))
            }
            "format" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("datetime.format() takes a timestamp and optional format string"));
                }
                let timestamp = self.datetime_number_arg(&args[0], "datetime.format() timestamp")?;
                let rendered = datetime_runtime::format(timestamp, self.datetime_format_arg(args.get(1), name)?)
                    .map_err(|e| self.err(&e))?;
                Ok(VmValue::Str(rendered))
            }
            "parse" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(self.err("datetime.parse() takes a text string and optional format string"));
                }
                let text = match &args[0] {
                    VmValue::Str(s) => s.as_str(),
                    other => {
                        return Err(self.err(&format!(
                            "datetime.parse() text must be a string, got {}",
                            other.type_name()
                        )))
                    }
                };
                let timestamp = datetime_runtime::parse(text, self.datetime_format_arg(args.get(1), name)?)
                    .map_err(|e| self.err(&e))?;
                Ok(VmValue::Float(timestamp))
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
                Ok(VmValue::Float(shifted))
            }
            "diff_seconds" => {
                if args.len() != 2 {
                    return Err(self.err("datetime.diff_seconds() takes two timestamp values"));
                }
                let left = self.datetime_number_arg(&args[0], "datetime.diff_seconds() left timestamp")?;
                let right = self.datetime_number_arg(&args[1], "datetime.diff_seconds() right timestamp")?;
                let diff = datetime_runtime::diff_seconds(left, right).map_err(|e| self.err(&e))?;
                Ok(VmValue::Float(diff))
            }
            _ => Err(self.err(&format!("unknown datetime function '{}'", name))),
        }
    }

    fn hashlib_text_arg<'a>(&self, value: &'a VmValue, context: &str) -> Result<&'a str, String> {
        match value {
            VmValue::Str(s) => Ok(s.as_str()),
            other => Err(self.err(&format!("{context} requires a string, got {}", other.type_name()))),
        }
    }

    fn call_hashlib_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
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
                Ok(VmValue::Str(digest))
            }
            "digest" => {
                if args.len() != 2 {
                    return Err(self.err("hashlib.digest() takes an algorithm name and text argument"));
                }
                let algo = self.hashlib_text_arg(&args[0], "hashlib.digest() algorithm")?;
                let text = self.hashlib_text_arg(&args[1], "hashlib.digest() text")?;
                let digest = hashlib_runtime::digest_hex(algo, text).map_err(|e| self.err(&e))?;
                Ok(VmValue::Str(digest))
            }
            _ => Err(self.err(&format!("unknown hashlib function '{}'", name))),
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

    fn call_path_module(&self, fname: &str, args: &[VmValue]) -> Result<VmValue, String> {
        let req_path_arg = |idx: usize, label: &str| -> Result<String, String> {
            match args.get(idx) {
                Some(VmValue::Str(s)) => Ok(s.clone()),
                _ => Err(self.err(label)),
            }
        };

        match fname {
            "join" => {
                let mut path = std::path::PathBuf::new();
                for arg in args {
                    let part = match arg {
                        VmValue::Str(s) => s.clone(),
                        _ => return Err(self.err("path.join requires string arguments")),
                    };
                    path.push(part);
                }
                Ok(VmValue::Str(path.to_string_lossy().to_string()))
            }
            "basename" => {
                let path = req_path_arg(0, "path.basename requires a path string")?;
                Ok(VmValue::Str(
                    std::path::Path::new(&path)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                ))
            }
            "dirname" => {
                let path = req_path_arg(0, "path.dirname requires a path string")?;
                let out = if path == "/" {
                    "/".to_string()
                } else {
                    std::path::Path::new(&path)
                        .parent()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                };
                Ok(VmValue::Str(out))
            }
            "ext" => {
                let path = req_path_arg(0, "path.ext requires a path string")?;
                Ok(VmValue::Str(
                    std::path::Path::new(&path)
                        .extension()
                        .map(|s| format!(".{}", s.to_string_lossy()))
                        .unwrap_or_default(),
                ))
            }
            "stem" => {
                let path = req_path_arg(0, "path.stem requires a path string")?;
                Ok(VmValue::Str(
                    std::path::Path::new(&path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                ))
            }
            "split" => {
                let path = req_path_arg(0, "path.split requires a path string")?;
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
                Ok(VmValue::List(Rc::new(RefCell::new(vec![
                    VmValue::Str(dir),
                    VmValue::Str(base),
                ]))))
            }
            "normalize" => {
                let path = req_path_arg(0, "path.normalize requires a path string")?;
                Ok(VmValue::Str(Self::normalize_path_string(&path)))
            }
            "exists" => {
                let path = req_path_arg(0, "path.exists requires a path string")?;
                Ok(VmValue::Bool(std::path::Path::new(&path).exists()))
            }
            "isabs" => {
                let path = req_path_arg(0, "path.isabs requires a path string")?;
                Ok(VmValue::Bool(std::path::Path::new(&path).is_absolute()))
            }
            _ => Err(self.err(&format!("unknown path function '{}'", fname))),
        }
    }

    fn call_platform_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        if !args.is_empty() {
            return Err(self.err(&format!("platform.{name}() takes no arguments")));
        }

        let value = match name {
            "os" => VmValue::Str(std::env::consts::OS.to_string()),
            "arch" => VmValue::Str(std::env::consts::ARCH.to_string()),
            "family" => VmValue::Str(std::env::consts::FAMILY.to_string()),
            "runtime" => VmValue::Str("vm".to_string()),
            "exe_ext" => VmValue::Str(std::env::consts::EXE_EXTENSION.to_string()),
            "shared_lib_ext" => VmValue::Str(
                if cfg!(target_os = "windows") {
                    "dll"
                } else if cfg!(target_os = "macos") {
                    "dylib"
                } else {
                    "so"
                }
                .to_string(),
            ),
            "path_sep" => VmValue::Str(std::path::MAIN_SEPARATOR.to_string()),
            "newline" => VmValue::Str(if cfg!(windows) { "\r\n" } else { "\n" }.to_string()),
            "is_windows" => VmValue::Bool(cfg!(windows)),
            "is_unix" => VmValue::Bool(cfg!(unix)),
            "has_ffi" => VmValue::Bool(false),
            "has_raw_memory" => VmValue::Bool(false),
            "has_extern" => VmValue::Bool(false),
            "has_inline_asm" => VmValue::Bool(false),
            _ => return Err(self.err(&format!("unknown platform function '{}'", name))),
        };
        Ok(value)
    }

    fn call_core_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
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
                Ok(VmValue::Int(core_runtime::word_bits()))
            }
            "word_bytes" => {
                if !args.is_empty() {
                    return Err(self.err("core.word_bytes() takes no arguments"));
                }
                Ok(VmValue::Int(core_runtime::word_bytes()))
            }
            "page_size" => {
                if !args.is_empty() {
                    return Err(self.err("core.page_size() takes no arguments"));
                }
                Ok(VmValue::Int(core_runtime::page_size()))
            }
            "page_align_down" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_align_down() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::page_align_down(req_int(
                    0,
                    "core.page_align_down",
                )?)))
            }
            "page_align_up" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_align_up() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::page_align_up(req_int(
                    0,
                    "core.page_align_up",
                )?)))
            }
            "page_offset" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_offset() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::page_offset(req_int(0, "core.page_offset")?)))
            }
            "page_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_index() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::page_index(req_int(0, "core.page_index")?)))
            }
            "page_count" => {
                if args.len() != 1 {
                    return Err(self.err("core.page_count() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::page_count(req_int(0, "core.page_count")?)))
            }
            "pt_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pt_index() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::pt_index(req_int(0, "core.pt_index")?)))
            }
            "pd_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pd_index() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::pd_index(req_int(0, "core.pd_index")?)))
            }
            "pdpt_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pdpt_index() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::pdpt_index(req_int(0, "core.pdpt_index")?)))
            }
            "pml4_index" => {
                if args.len() != 1 {
                    return Err(self.err("core.pml4_index() takes exactly one argument"));
                }
                Ok(VmValue::Int(core_runtime::pml4_index(req_int(0, "core.pml4_index")?)))
            }
            "alloc" | "free" | "set_allocator" | "clear_allocator" => Err(self.err(&format!(
                "core.{name}() is only supported in the LLVM backend — compile with `cool build`"
            ))),
            _ => Err(self.err(&format!("unknown core function '{}'", name))),
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn vm_len(&self, v: &VmValue) -> Result<usize, String> {
        match v {
            VmValue::List(l) => Ok(l.borrow().len()),
            VmValue::Str(s) => Ok(s.chars().count()),
            VmValue::Tuple(t) => Ok(t.len()),
            VmValue::Dict(d) => Ok(d.borrow().keys.len()),
            other => Err(self.err(&format!("len() not supported for {}", other.type_name()))),
        }
    }

    fn coerce_to_int(&self, value: &VmValue) -> Result<i64, String> {
        match value {
            VmValue::Int(n) => Ok(*n),
            VmValue::Float(f) => Ok(*f as i64),
            VmValue::Bool(b) => Ok(if *b { 1 } else { 0 }),
            VmValue::Str(s) => {
                let s = s.trim();
                if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                    i64::from_str_radix(hex, 16).map_err(|_| self.err(&format!("invalid int: '{}'", s)))
                } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                    i64::from_str_radix(bin, 2).map_err(|_| self.err(&format!("invalid int: '{}'", s)))
                } else {
                    s.parse::<i64>().map_err(|_| self.err(&format!("invalid int: '{}'", s)))
                }
            }
            other => Err(self.err(&format!("cannot convert {} to int", other.type_name()))),
        }
    }

    fn builtin_min_max(&self, args: &[VmValue], is_max: bool) -> Result<VmValue, String> {
        let items: Vec<VmValue> = if args.len() == 1 {
            self.to_iter_vec(args[0].clone())?
        } else {
            args.to_vec()
        };
        if items.is_empty() {
            return Err(self.err(if is_max {
                "max() of empty sequence"
            } else {
                "min() of empty sequence"
            }));
        }
        let mut best = items[0].clone();
        for item in &items[1..] {
            let cmp = vm_cmp_order(&best, item);
            if (is_max && cmp == std::cmp::Ordering::Less) || (!is_max && cmp == std::cmp::Ordering::Greater) {
                best = item.clone();
            }
        }
        Ok(best)
    }

    fn is_instance_of(&self, cls: &Rc<VmClass>, target: &Rc<VmClass>) -> bool {
        if std::ptr::eq(cls.as_ref(), target.as_ref()) || cls.name == target.name {
            return true;
        }
        if let Some(parent) = &cls.parent {
            return self.is_instance_of(parent, target);
        }
        false
    }

    fn run_file(&mut self, path: &str, extra_args: &[String]) -> Result<VmValue, String> {
        let full = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            self.source_dir.join(path)
        };
        let source = std::fs::read_to_string(&full).map_err(|e| self.err(&format!("runfile '{}': {}", path, e)))?;
        let mut lexer = crate::lexer::Lexer::new(&source);
        let tokens = lexer.tokenize().map_err(|e| self.err(&e))?;
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse_program().map_err(|e| self.err(&e))?;
        let chunk = crate::compiler::compile(&program).map_err(|e| self.err(&e))?;
        let old_source_dir = self.source_dir.clone();
        let old_script_path = std::env::var("COOL_SCRIPT_PATH").ok();
        let old_program_args = std::env::var("COOL_PROGRAM_ARGS").ok();
        if let Some(parent) = full.parent() {
            self.source_dir = parent.to_path_buf();
        }
        std::env::set_var("COOL_SCRIPT_PATH", full.to_string_lossy().to_string());
        if !extra_args.is_empty() {
            std::env::set_var("COOL_PROGRAM_ARGS", extra_args.join("\x1F"));
        } else {
            std::env::remove_var("COOL_PROGRAM_ARGS");
        }
        self.run(&chunk)?;
        self.source_dir = old_source_dir;
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
        Ok(VmValue::Nil)
    }

    // ── JSON helpers ──────────────────────────────────────────────────────────

    fn json_loads(&mut self, s: &str) -> Result<VmValue, String> {
        let s = s.trim();
        if s == "null" {
            return Ok(VmValue::Nil);
        }
        if s == "true" {
            return Ok(VmValue::Bool(true));
        }
        if s == "false" {
            return Ok(VmValue::Bool(false));
        }
        if let Ok(i) = s.parse::<i64>() {
            return Ok(VmValue::Int(i));
        }
        if let Ok(f) = s.parse::<f64>() {
            return Ok(VmValue::Float(f));
        }
        if s.starts_with('"') && s.ends_with('"') {
            let inner = &s[1..s.len() - 1];
            return Ok(VmValue::Str(
                inner
                    .replace("\\n", "\n")
                    .replace("\\t", "\t")
                    .replace("\\\"", "\"")
                    .replace("\\\\", "\\"),
            ));
        }
        if s.starts_with('[') && s.ends_with(']') {
            let inner = &s[1..s.len() - 1].trim();
            let mut items = Vec::new();
            if !inner.is_empty() {
                for item in Self::json_split_array(inner) {
                    items.push(self.json_loads(item.trim())?);
                }
            }
            return Ok(VmValue::List(Rc::new(RefCell::new(items))));
        }
        if s.starts_with('{') && s.ends_with('}') {
            let inner = s[1..s.len() - 1].trim();
            let mut d = VmDict::new();
            if !inner.is_empty() {
                for pair in Self::json_split_array(inner) {
                    let pair = pair.trim();
                    if let Some(colon) = Self::json_find_colon(pair) {
                        let key = self.json_loads(pair[..colon].trim())?;
                        let val = self.json_loads(pair[colon + 1..].trim())?;
                        d.set(key, val);
                    }
                }
            }
            return Ok(VmValue::Dict(Rc::new(RefCell::new(d))));
        }
        Err(self.err(&format!("json.loads: cannot parse: {}", &s[..s.len().min(40)])))
    }

    fn json_split_array(s: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut depth = 0i32;
        let mut in_str = false;
        let mut start = 0;
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'"' if !in_str => in_str = true,
                b'"' if in_str && (i == 0 || bytes[i - 1] != b'\\') => in_str = false,
                b'[' | b'{' if !in_str => depth += 1,
                b']' | b'}' if !in_str => depth -= 1,
                b',' if !in_str && depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        parts.push(&s[start..]);
        parts
    }

    fn json_find_colon(s: &str) -> Option<usize> {
        let mut in_str = false;
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'"' if !in_str => in_str = true,
                b'"' if in_str && (i == 0 || bytes[i - 1] != b'\\') => in_str = false,
                b':' if !in_str => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn json_dumps(&self, v: &VmValue) -> String {
        match v {
            VmValue::Nil => "null".to_string(),
            VmValue::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            VmValue::Int(i) => i.to_string(),
            VmValue::Float(f) => format!("{}", f),
            VmValue::Str(s) => format!(
                "\"{}\"",
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\t', "\\t")
            ),
            VmValue::List(v) => {
                let items: Vec<String> = v.borrow().iter().map(|x| self.json_dumps(x)).collect();
                format!("[{}]", items.join(", "))
            }
            VmValue::Dict(d) => {
                let db = d.borrow();
                let pairs: Vec<String> = db
                    .keys
                    .iter()
                    .zip(db.vals.iter())
                    .map(|(k, val)| format!("{}: {}", self.json_dumps(k), self.json_dumps(val)))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            }
            other => format!("\"{}\"", other.to_string().replace('"', "\\\"")),
        }
    }

    fn vm_value_to_toml_data(&self, value: &VmValue) -> Result<TomlData, String> {
        match value {
            VmValue::Int(n) => Ok(TomlData::Int(*n)),
            VmValue::Float(f) if f.is_finite() => Ok(TomlData::Float(*f)),
            VmValue::Float(_) => Err(self.err("toml.dumps() does not support NaN or infinite floats")),
            VmValue::Str(s) => Ok(TomlData::Str(s.clone())),
            VmValue::Bool(b) => Ok(TomlData::Bool(*b)),
            VmValue::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.vm_value_to_toml_data(item)?);
                }
                Ok(TomlData::List(out))
            }
            VmValue::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.vm_value_to_toml_data(item)?);
                }
                Ok(TomlData::List(out))
            }
            VmValue::Dict(map) => {
                let map = map.borrow();
                let mut out = Vec::with_capacity(map.keys.len());
                for (key, value) in map.keys.iter().zip(map.vals.iter()) {
                    let key = match key {
                        VmValue::Str(s) => s.clone(),
                        other => {
                            return Err(self.err(&format!(
                                "toml.dumps() dict keys must be strings, got {}",
                                other.type_name()
                            )))
                        }
                    };
                    out.push((key, self.vm_value_to_toml_data(value)?));
                }
                Ok(TomlData::Dict(out))
            }
            other => Err(self.err(&format!(
                "toml.dumps() only supports ints/floats/strings/bools/lists/tuples/dicts, got {}",
                other.type_name()
            ))),
        }
    }

    fn toml_data_to_vm_value(data: &TomlData) -> VmValue {
        match data {
            TomlData::Int(n) => VmValue::Int(*n),
            TomlData::Float(f) => VmValue::Float(*f),
            TomlData::Str(s) => VmValue::Str(s.clone()),
            TomlData::Bool(b) => VmValue::Bool(*b),
            TomlData::List(items) => VmValue::List(Rc::new(RefCell::new(
                items.iter().map(Self::toml_data_to_vm_value).collect(),
            ))),
            TomlData::Dict(items) => {
                let mut out = VmDict::new();
                for (key, value) in items {
                    out.set(VmValue::Str(key.clone()), Self::toml_data_to_vm_value(value));
                }
                VmValue::Dict(Rc::new(RefCell::new(out)))
            }
        }
    }

    fn call_toml_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "loads" => {
                let s = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("toml.loads requires a string")),
                };
                Ok(Self::toml_data_to_vm_value(
                    &toml_runtime::loads(&s).map_err(|e| self.err(&e))?,
                ))
            }
            "dumps" => {
                let value = match args.first() {
                    Some(v) => v,
                    None => return Err(self.err("toml.dumps requires a value")),
                };
                Ok(VmValue::Str(
                    toml_runtime::dumps(&self.vm_value_to_toml_data(value)?).map_err(|e| self.err(&e))?,
                ))
            }
            _ => Err(self.err(&format!("unknown toml function '{}'", name))),
        }
    }

    fn vm_value_to_yaml_data(&self, value: &VmValue) -> Result<YamlData, String> {
        match value {
            VmValue::Nil => Ok(YamlData::Nil),
            VmValue::Int(n) => Ok(YamlData::Int(*n)),
            VmValue::Float(f) if f.is_finite() => Ok(YamlData::Float(*f)),
            VmValue::Float(_) => Err(self.err("yaml.dumps() does not support NaN or infinite floats")),
            VmValue::Str(s) => Ok(YamlData::Str(s.clone())),
            VmValue::Bool(b) => Ok(YamlData::Bool(*b)),
            VmValue::List(items) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.vm_value_to_yaml_data(item)?);
                }
                Ok(YamlData::List(out))
            }
            VmValue::Tuple(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.vm_value_to_yaml_data(item)?);
                }
                Ok(YamlData::List(out))
            }
            VmValue::Dict(map) => {
                let map = map.borrow();
                let mut out = Vec::with_capacity(map.keys.len());
                for (key, value) in map.keys.iter().zip(map.vals.iter()) {
                    let key = match key {
                        VmValue::Str(s) => s.clone(),
                        other => {
                            return Err(self.err(&format!(
                                "yaml.dumps() dict keys must be strings, got {}",
                                other.type_name()
                            )))
                        }
                    };
                    out.push((key, self.vm_value_to_yaml_data(value)?));
                }
                Ok(YamlData::Dict(out))
            }
            other => Err(self.err(&format!(
                "yaml.dumps() only supports nil/ints/floats/strings/bools/lists/tuples/dicts, got {}",
                other.type_name()
            ))),
        }
    }

    fn yaml_data_to_vm_value(data: &YamlData) -> VmValue {
        match data {
            YamlData::Nil => VmValue::Nil,
            YamlData::Int(n) => VmValue::Int(*n),
            YamlData::Float(f) => VmValue::Float(*f),
            YamlData::Str(s) => VmValue::Str(s.clone()),
            YamlData::Bool(b) => VmValue::Bool(*b),
            YamlData::List(items) => VmValue::List(Rc::new(RefCell::new(
                items.iter().map(Self::yaml_data_to_vm_value).collect(),
            ))),
            YamlData::Dict(items) => {
                let mut out = VmDict::new();
                for (key, value) in items {
                    out.set(VmValue::Str(key.clone()), Self::yaml_data_to_vm_value(value));
                }
                VmValue::Dict(Rc::new(RefCell::new(out)))
            }
        }
    }

    fn call_yaml_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "loads" => {
                let s = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    _ => return Err(self.err("yaml.loads requires a string")),
                };
                Ok(Self::yaml_data_to_vm_value(
                    &yaml_runtime::loads(&s).map_err(|e| self.err(&e))?,
                ))
            }
            "dumps" => {
                let value = match args.first() {
                    Some(v) => v,
                    None => return Err(self.err("yaml.dumps requires a value")),
                };
                Ok(VmValue::Str(
                    yaml_runtime::dumps(&self.vm_value_to_yaml_data(value)?).map_err(|e| self.err(&e))?,
                ))
            }
            _ => Err(self.err(&format!("unknown yaml function '{}'", name))),
        }
    }

    fn vm_value_to_sql_data(&self, value: &VmValue) -> Result<SqlData, String> {
        match value {
            VmValue::Nil => Ok(SqlData::Nil),
            VmValue::Int(n) => Ok(SqlData::Int(*n)),
            VmValue::Float(f) if f.is_finite() => Ok(SqlData::Float(*f)),
            VmValue::Float(_) => Err(self.err("sqlite parameters do not support NaN or infinite floats")),
            VmValue::Str(s) => Ok(SqlData::Str(s.clone())),
            VmValue::Bool(b) => Ok(SqlData::Bool(*b)),
            other => Err(self.err(&format!(
                "sqlite parameters only support nil/int/float/str/bool, got {}",
                other.type_name()
            ))),
        }
    }

    fn sqlite_data_to_vm_value(data: &SqlData) -> VmValue {
        match data {
            SqlData::Nil => VmValue::Nil,
            SqlData::Int(n) => VmValue::Int(*n),
            SqlData::Float(f) => VmValue::Float(*f),
            SqlData::Str(s) => VmValue::Str(s.clone()),
            SqlData::Bool(b) => VmValue::Bool(*b),
        }
    }

    fn sqlite_params_arg(&self, value: Option<&VmValue>) -> Result<Vec<SqlData>, String> {
        match value {
            None | Some(VmValue::Nil) => Ok(Vec::new()),
            Some(VmValue::List(items)) => {
                let mut out = Vec::with_capacity(items.borrow().len());
                for item in items.borrow().iter() {
                    out.push(self.vm_value_to_sql_data(item)?);
                }
                Ok(out)
            }
            Some(VmValue::Tuple(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items.iter() {
                    out.push(self.vm_value_to_sql_data(item)?);
                }
                Ok(out)
            }
            Some(other) => Err(self.err(&format!(
                "sqlite params must be a list, tuple, or nil, got {}",
                other.type_name()
            ))),
        }
    }

    fn sqlite_rows_to_vm_value(rows: Vec<Vec<(String, SqlData)>>) -> VmValue {
        let rows = rows
            .into_iter()
            .map(|row| {
                let mut dict = VmDict::new();
                for (key, value) in row {
                    dict.set(VmValue::Str(key), Self::sqlite_data_to_vm_value(&value));
                }
                VmValue::Dict(Rc::new(RefCell::new(dict)))
            })
            .collect();
        VmValue::List(Rc::new(RefCell::new(rows)))
    }

    fn call_sqlite_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        let path = match args.first() {
            Some(VmValue::Str(s)) => s.clone(),
            other => {
                return Err(self.err(&format!(
                    "sqlite.{} requires a path string, got {}",
                    name,
                    other.map_or("nil", VmValue::type_name)
                )))
            }
        };
        let sql = match args.get(1) {
            Some(VmValue::Str(s)) => s.clone(),
            other => {
                return Err(self.err(&format!(
                    "sqlite.{} requires a SQL string, got {}",
                    name,
                    other.map_or("nil", VmValue::type_name)
                )))
            }
        };
        let params = self.sqlite_params_arg(args.get(2))?;
        match name {
            "execute" => Ok(VmValue::Int(
                sqlite_runtime::execute(&path, &sql, &params).map_err(|e| self.err(&e))?,
            )),
            "query" => Ok(Self::sqlite_rows_to_vm_value(
                sqlite_runtime::query(&path, &sql, &params).map_err(|e| self.err(&e))?,
            )),
            "scalar" => Ok(Self::sqlite_data_to_vm_value(
                &sqlite_runtime::scalar(&path, &sql, &params).map_err(|e| self.err(&e))?,
            )),
            _ => Err(self.err(&format!("unknown sqlite function '{}'", name))),
        }
    }

    fn http_headers_arg(&self, value: Option<&VmValue>, context: &str) -> Result<Vec<String>, String> {
        match value {
            None | Some(VmValue::Nil) => Ok(Vec::new()),
            Some(VmValue::List(items)) => items
                .borrow()
                .iter()
                .map(|item| match item {
                    VmValue::Str(s) => Ok(s.clone()),
                    other => Err(self.err(&format!(
                        "{context} headers must contain only strings, got {}",
                        other.type_name()
                    ))),
                })
                .collect(),
            Some(VmValue::Tuple(items)) => items
                .iter()
                .map(|item| match item {
                    VmValue::Str(s) => Ok(s.clone()),
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

    fn call_http_module(&mut self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "get" => {
                let url = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    other => {
                        return Err(self.err(&format!(
                            "http.get requires a URL string, got {}",
                            other.map_or("nil", VmValue::type_name)
                        )))
                    }
                };
                let headers = self.http_headers_arg(args.get(1), "http.get()")?;
                Ok(VmValue::Str(
                    http_runtime::get(&url, &headers).map_err(|e| self.err(&e))?,
                ))
            }
            "post" => {
                let url = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    other => {
                        return Err(self.err(&format!(
                            "http.post requires a URL string, got {}",
                            other.map_or("nil", VmValue::type_name)
                        )))
                    }
                };
                let data = match args.get(1) {
                    Some(VmValue::Str(s)) => s.clone(),
                    other => {
                        return Err(self.err(&format!(
                            "http.post requires a body string, got {}",
                            other.map_or("nil", VmValue::type_name)
                        )))
                    }
                };
                let headers = self.http_headers_arg(args.get(2), "http.post()")?;
                Ok(VmValue::Str(
                    http_runtime::post(&url, &data, &headers).map_err(|e| self.err(&e))?,
                ))
            }
            "head" => {
                let url = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    other => {
                        return Err(self.err(&format!(
                            "http.head requires a URL string, got {}",
                            other.map_or("nil", VmValue::type_name)
                        )))
                    }
                };
                let headers = self.http_headers_arg(args.get(1), "http.head()")?;
                Ok(VmValue::Str(
                    http_runtime::head(&url, &headers).map_err(|e| self.err(&e))?,
                ))
            }
            "getjson" => {
                let url = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    other => {
                        return Err(self.err(&format!(
                            "http.getjson requires a URL string, got {}",
                            other.map_or("nil", VmValue::type_name)
                        )))
                    }
                };
                let headers = self.http_headers_arg(args.get(1), "http.getjson()")?;
                let body = http_runtime::getjson(&url, &headers).map_err(|e| self.err(&e))?;
                self.json_loads(&body)
                    .map_err(|e| self.err(&format!("http.getjson invalid JSON: {}", e)))
            }
            _ => Err(self.err(&format!("unknown http function '{}'", name))),
        }
    }

    fn call_term_module(&self, name: &str, args: &[VmValue]) -> Result<VmValue, String> {
        match name {
            "raw" => {
                terminal::enable_raw_mode().map_err(|e| self.err(&format!("term.raw() error: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "normal" => {
                terminal::disable_raw_mode().map_err(|e| self.err(&format!("term.normal() error: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "clear" => {
                execute!(
                    std::io::stdout(),
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                    crossterm::cursor::MoveTo(0, 0)
                )
                .map_err(|e| self.err(&format!("term.clear() error: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "clear_line" => {
                execute!(
                    std::io::stdout(),
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine)
                )
                .map_err(|e| self.err(&format!("term.clear_line() error: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "move_cursor" => {
                let row = match args.first() {
                    Some(VmValue::Int(n)) => (*n as u16).saturating_sub(1),
                    _ => return Err(self.err("term.move_cursor(row, col) requires integers")),
                };
                let col = match args.get(1) {
                    Some(VmValue::Int(n)) => (*n as u16).saturating_sub(1),
                    _ => return Err(self.err("term.move_cursor(row, col) requires integers")),
                };
                execute!(std::io::stdout(), crossterm::cursor::MoveTo(col, row))
                    .map_err(|e| self.err(&format!("term.move_cursor() error: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "hide_cursor" => {
                execute!(std::io::stdout(), crossterm::cursor::Hide)
                    .map_err(|e| self.err(&format!("term.hide_cursor() error: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "show_cursor" => {
                execute!(std::io::stdout(), crossterm::cursor::Show)
                    .map_err(|e| self.err(&format!("term.show_cursor() error: {}", e)))?;
                Ok(VmValue::Nil)
            }
            "write" => {
                let s = match args.first() {
                    Some(VmValue::Str(s)) => s.clone(),
                    Some(v) => v.to_string(),
                    None => return Ok(VmValue::Nil),
                };
                print!("{}", s);
                std::io::stdout().flush().ok();
                Ok(VmValue::Nil)
            }
            "flush" => {
                std::io::stdout().flush().ok();
                Ok(VmValue::Nil)
            }
            "size" => {
                let (w, h) = terminal::size().map_err(|e| self.err(&format!("term.size() error: {}", e)))?;
                Ok(VmValue::Tuple(Rc::new(vec![
                    VmValue::Int(w as i64),
                    VmValue::Int(h as i64),
                ])))
            }
            "get_char" => loop {
                if let Ok(Event::Key(key)) = ct_event::read() {
                    return Ok(VmValue::Str(vm_key_to_string(key)));
                }
            },
            "poll_char" => {
                let ms = match args.first() {
                    Some(VmValue::Int(n)) => *n as u64,
                    Some(VmValue::Float(f)) => *f as u64,
                    None => 0,
                    _ => return Err(self.err("term.poll_char(ms) requires a number")),
                };
                if ct_event::poll(std::time::Duration::from_millis(ms))
                    .map_err(|e| self.err(&format!("term.poll_char() error: {}", e)))?
                {
                    if let Ok(Event::Key(key)) = ct_event::read() {
                        return Ok(VmValue::Str(vm_key_to_string(key)));
                    }
                }
                Ok(VmValue::Nil)
            }
            _ => Err(self.err(&format!("unknown term function '{}'", name))),
        }
    }

    fn eval_source(&mut self, src: &str) -> Result<VmValue, String> {
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().map_err(|e| self.err(&e))?;
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse_program().map_err(|e| self.err(&e))?;
        let chunk = crate::compiler::compile(&program).map_err(|e| self.err(&e))?;
        self.run(&chunk)?;
        Ok(VmValue::Nil)
    }

    fn import_file(&mut self, path: &str) -> Result<VmValue, String> {
        let full_path = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            self.source_dir.join(path)
        };
        let canonical = full_path.canonicalize().unwrap_or(full_path.clone());
        if self.importing_modules.contains(&canonical) {
            return Err(self.err(&format!("circular import detected for '{}'", canonical.display())));
        }
        self.importing_modules.push(canonical.clone());
        let source = std::fs::read_to_string(&canonical).map_err(|e| self.err(&format!("import {}: {}", path, e)))?;
        let module_dir = canonical.parent().unwrap_or(&self.source_dir).to_path_buf();
        let mut module_vm = VM::new(module_dir, self.module_resolver.clone());
        module_vm.importing_modules = self.importing_modules.clone();

        let mut lexer = crate::lexer::Lexer::new(&source);
        let tokens = match lexer.tokenize().map_err(|e| self.err(&e)) {
            Ok(tokens) => tokens,
            Err(err) => {
                self.importing_modules.pop();
                return Err(err);
            }
        };
        let mut parser = crate::parser::Parser::new(tokens);
        let program = match parser.parse_program().map_err(|e| self.err(&e)) {
            Ok(program) => program,
            Err(err) => {
                self.importing_modules.pop();
                return Err(err);
            }
        };
        let exported_names: std::collections::HashSet<String> =
            module_exports::exported_names(&program).into_iter().collect();
        let chunk = match crate::compiler::compile(&program).map_err(|e| self.err(&e)) {
            Ok(chunk) => chunk,
            Err(err) => {
                self.importing_modules.pop();
                return Err(err);
            }
        };
        if let Err(err) = module_vm.run(&chunk) {
            self.importing_modules.pop();
            return Err(err);
        }
        self.importing_modules.pop();
        for name in &exported_names {
            if let Some(value) = module_vm.globals.get(name) {
                self.globals.insert(name.clone(), value.clone());
            }
        }
        Ok(VmValue::Nil)
    }

    fn import_module(&mut self, name: &str) -> Result<VmValue, String> {
        // Try to load from source dir first (e.g., math.cool, foo/bar.cool).
        if let Some(path) = self.module_resolver.resolve_module(&self.source_dir, name) {
            let module_path = path.canonicalize().unwrap_or_else(|_| path.clone());
            if self.importing_modules.contains(&module_path) {
                return Err(self.err(&format!("circular import detected for '{}'", module_path.display())));
            }
            self.importing_modules.push(module_path);
            let source = std::fs::read_to_string(&path).map_err(|e| self.err(&format!("import {}: {}", name, e)))?;
            // Run the module in an isolated VM so imports expose a namespace
            // instead of leaking module locals into the caller's globals.
            let module_dir = path.parent().unwrap_or(&self.source_dir).to_path_buf();
            let mut module_vm = VM::new(module_dir, self.module_resolver.clone());
            module_vm.importing_modules = self.importing_modules.clone();
            let mut lexer = crate::lexer::Lexer::new(&source);
            let tokens = match lexer.tokenize().map_err(|e| self.err(&e)) {
                Ok(tokens) => tokens,
                Err(err) => {
                    self.importing_modules.pop();
                    return Err(err);
                }
            };
            let mut parser = crate::parser::Parser::new(tokens);
            let program = match parser.parse_program().map_err(|e| self.err(&e)) {
                Ok(program) => program,
                Err(err) => {
                    self.importing_modules.pop();
                    return Err(err);
                }
            };
            let exported_names: std::collections::HashSet<String> =
                module_exports::exported_names(&program).into_iter().collect();
            let chunk = match crate::compiler::compile(&program).map_err(|e| self.err(&e)) {
                Ok(chunk) => chunk,
                Err(err) => {
                    self.importing_modules.pop();
                    return Err(err);
                }
            };
            if let Err(err) = module_vm.run(&chunk) {
                self.importing_modules.pop();
                return Err(err);
            }
            self.importing_modules.pop();
            // Return the module's exported namespace.
            let mut d = VmDict::new();
            for name in &exported_names {
                if let Some(value) = module_vm.globals.get(name) {
                    d.set(VmValue::Str(name.clone()), value.clone());
                }
            }
            return Ok(VmValue::Dict(Rc::new(RefCell::new(d))));
        }
        // Fall back: built-in stub modules.
        self.make_builtin_module(name)
    }

    fn make_builtin_module(&mut self, name: &str) -> Result<VmValue, String> {
        let mut d = VmDict::new();
        let set = |d: &mut VmDict, k: &str, v: VmValue| d.set(VmValue::Str(k.to_string()), v);
        let bf = |n: &str| VmValue::BuiltinFn(n.to_string());

        match name {
            "math" => {
                set(&mut d, "pi", VmValue::Float(std::f64::consts::PI));
                set(&mut d, "e", VmValue::Float(std::f64::consts::E));
                set(&mut d, "tau", VmValue::Float(std::f64::consts::TAU));
                for fname in &[
                    "sqrt",
                    "floor",
                    "ceil",
                    "round",
                    "log",
                    "log2",
                    "log10",
                    "sin",
                    "cos",
                    "tan",
                    "asin",
                    "acos",
                    "atan",
                    "atan2",
                    "degrees",
                    "radians",
                    "exp",
                    "exp2",
                    "abs",
                    "pow",
                    "hypot",
                    "gcd",
                    "lcm",
                    "factorial",
                    "trunc",
                    "sinh",
                    "cosh",
                    "tanh",
                    "isnan",
                    "isinf",
                    "isfinite",
                ] {
                    set(&mut d, fname, bf(&format!("math.{}", fname)));
                }
                self.globals.extend([
                    ("math.sqrt".to_string(), bf("math.sqrt")),
                    ("math.floor".to_string(), bf("math.floor")),
                    ("math.ceil".to_string(), bf("math.ceil")),
                    ("math.pi".to_string(), VmValue::Float(std::f64::consts::PI)),
                    ("math.e".to_string(), VmValue::Float(std::f64::consts::E)),
                    ("math.tau".to_string(), VmValue::Float(std::f64::consts::TAU)),
                ]);
            }
            "os" => {
                for fname in &[
                    "listdir", "mkdir", "remove", "rename", "exists", "isdir", "getenv", "getcwd", "join", "path",
                    "popen",
                ] {
                    set(&mut d, fname, bf(&format!("os.{}", fname)));
                }
            }
            "sys" => {
                let mut argv: Vec<VmValue> = Vec::new();
                if let Ok(script_path) = std::env::var("COOL_SCRIPT_PATH") {
                    argv.push(VmValue::Str(script_path));
                } else {
                    argv.extend(std::env::args().map(VmValue::Str));
                }
                if let Ok(extra) = std::env::var("COOL_PROGRAM_ARGS") {
                    if !extra.is_empty() {
                        for arg in extra.split("\x1F") {
                            argv.push(VmValue::Str(arg.to_string()));
                        }
                    }
                }
                set(&mut d, "argv", VmValue::List(Rc::new(RefCell::new(argv))));
                set(&mut d, "exit", bf("exit"));
            }
            "path" => {
                for fname in &[
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
                    set(&mut d, fname, bf(&format!("path.{}", fname)));
                }
            }
            "platform" => {
                for fname in &[
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
                    set(&mut d, fname, bf(&format!("platform.{}", fname)));
                }
            }
            "core" => {
                for fname in &[
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
                    set(&mut d, fname, bf(&format!("core.{}", fname)));
                }
            }
            "subprocess" => {
                for fname in &["run", "call", "check_output"] {
                    set(&mut d, fname, bf(&format!("subprocess.{}", fname)));
                }
            }
            "argparse" => {
                for fname in &["parse", "help"] {
                    set(&mut d, fname, bf(&format!("argparse.{}", fname)));
                }
            }
            "csv" => {
                for fname in &["rows", "dicts", "write"] {
                    set(&mut d, fname, bf(&format!("csv.{}", fname)));
                }
            }
            "datetime" => {
                for fname in &["now", "format", "parse", "parts", "add_seconds", "diff_seconds"] {
                    set(&mut d, fname, bf(&format!("datetime.{}", fname)));
                }
            }
            "hashlib" => {
                for fname in &["md5", "sha1", "sha256", "digest"] {
                    set(&mut d, fname, bf(&format!("hashlib.{}", fname)));
                }
            }
            "test" => {
                for fname in &[
                    "equal",
                    "not_equal",
                    "truthy",
                    "falsey",
                    "is_nil",
                    "not_nil",
                    "fail",
                    "raises",
                ] {
                    set(&mut d, fname, bf(&format!("test.{}", fname)));
                }
            }
            "logging" => {
                for fname in &["basic_config", "log", "debug", "info", "warning", "warn", "error"] {
                    set(&mut d, fname, bf(&format!("logging.{}", fname)));
                }
            }
            "time" => {
                for fname in &["time", "sleep", "monotonic"] {
                    set(&mut d, fname, bf(&format!("time.{}", fname)));
                }
            }
            "random" => {
                for fname in &["random", "randint", "choice", "shuffle", "uniform", "seed"] {
                    set(&mut d, fname, bf(&format!("random.{}", fname)));
                }
            }
            "json" => {
                set(&mut d, "loads", bf("json.loads"));
                set(&mut d, "dumps", bf("json.dumps"));
            }
            "toml" => {
                set(&mut d, "loads", bf("toml.loads"));
                set(&mut d, "dumps", bf("toml.dumps"));
            }
            "yaml" => {
                set(&mut d, "loads", bf("yaml.loads"));
                set(&mut d, "dumps", bf("yaml.dumps"));
            }
            "sqlite" => {
                for fname in &["execute", "query", "scalar"] {
                    set(&mut d, fname, bf(&format!("sqlite.{}", fname)));
                }
            }
            "http" => {
                for fname in &["get", "post", "head", "getjson"] {
                    set(&mut d, fname, bf(&format!("http.{}", fname)));
                }
            }
            "socket" => {
                for fname in &["connect", "listen"] {
                    set(&mut d, fname, bf(&format!("socket.{}", fname)));
                }
            }
            "term" => {
                for fname in &[
                    "raw",
                    "normal",
                    "clear",
                    "clear_line",
                    "move_cursor",
                    "hide_cursor",
                    "show_cursor",
                    "write",
                    "flush",
                    "size",
                    "get_char",
                    "poll_char",
                ] {
                    set(&mut d, fname, bf(&format!("term.{}", fname)));
                }
            }
            "re" => {
                for fname in &["match", "search", "fullmatch", "findall", "sub", "split"] {
                    set(&mut d, fname, bf(&format!("re.{}", fname)));
                }
            }
            "string" => {
                for fname in &[
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
                    set(&mut d, fname, bf(&format!("stringmod.{}", fname)));
                }
            }
            "list" => {
                for fname in &["sort", "reverse", "filter", "map", "reduce", "flatten", "unique"] {
                    set(&mut d, fname, bf(&format!("listmod.{}", fname)));
                }
            }
            "collections" => {
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
                let before = self.globals.clone();
                self.eval_source(src)?;
                if let Some(queue) = self.globals.get("Queue").cloned() {
                    set(&mut d, "Queue", queue);
                }
                if let Some(stack) = self.globals.get("Stack").cloned() {
                    set(&mut d, "Stack", stack);
                }
                self.globals = before;
            }
            "ffi" => {
                set(&mut d, "open", bf("ffi.open"));
                set(&mut d, "func", bf("ffi.func"));
            }
            _ => return Err(self.err(&format!("unknown module '{}'", name))),
        }
        Ok(VmValue::Dict(Rc::new(RefCell::new(d))))
    }
}

// ── Ordering helper ───────────────────────────────────────────────────────────

fn vm_cmp_order(a: &VmValue, b: &VmValue) -> std::cmp::Ordering {
    match (a, b) {
        (VmValue::Int(x), VmValue::Int(y)) => x.cmp(y),
        (VmValue::Float(x), VmValue::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (VmValue::Int(x), VmValue::Float(y)) => (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (VmValue::Float(x), VmValue::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal),
        (VmValue::Str(x), VmValue::Str(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    }
}

fn vm_key_to_string(key: KeyEvent) -> String {
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

// ── Slice index resolution ────────────────────────────────────────────────────

fn resolve_slice_idx(idx: Option<i64>, len: i64, default: i64) -> i64 {
    match idx {
        None => default,
        Some(i) => {
            let i = if i < 0 { len + i } else { i };
            i.max(0).min(len)
        }
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
