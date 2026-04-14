/// Compile a Cool AST into bytecode for the stack-based VM.
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ast::*;
use crate::opcode::*;

// ── Variable resolution ───────────────────────────────────────────────────────

/// One entry in the local-variable table for the current function scope.
#[derive(Debug, Clone)]
struct Local {
    name: String,
    /// Stack slot index within the current function frame.
    slot: usize,
    /// Is this local captured by any nested closure?
    is_captured: bool,
}

/// Upvalue entry recorded during compilation.
#[derive(Debug, Clone)]
struct Upvalue {
    name: String,
    capture: UpvalueRef,
}

/// Represents one function being compiled (nested functions each get their own scope).
struct FnScope {
    locals: Vec<Local>,
    upvalues: Vec<Upvalue>,
    /// Variables declared `global` in this function.
    globals_declared: HashSet<String>,
    /// Variables declared `nonlocal` in this function.
    nonlocals_declared: HashSet<String>,
    /// Next available local slot.
    next_slot: usize,
    /// Current nesting depth of break/continue-able loops.
    loop_stack: Vec<LoopInfo>,
}

struct LoopInfo {
    /// Indices of `Jump(usize::MAX)` break-placeholder instructions.
    break_patches: Vec<usize>,
    /// Instruction index to jump back to for `continue`.
    continue_target: usize,
}

impl FnScope {
    fn new() -> Self {
        FnScope {
            locals: Vec::new(),
            upvalues: Vec::new(),
            globals_declared: HashSet::new(),
            nonlocals_declared: HashSet::new(),
            next_slot: 0,
            loop_stack: Vec::new(),
        }
    }

    fn add_local(&mut self, name: &str) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.locals.push(Local { name: name.to_string(), slot, is_captured: false });
        slot
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        // Search from the end so inner-scope locals shadow outer ones.
        self.locals.iter().rev().find(|l| l.name == name).map(|l| l.slot)
    }

    fn add_upvalue(&mut self, name: &str, capture: UpvalueRef) -> usize {
        // Check if already captured.
        if let Some(i) = self.upvalues.iter().position(|u| u.name == name) {
            return i;
        }
        let idx = self.upvalues.len();
        self.upvalues.push(Upvalue { name: name.to_string(), capture });
        idx
    }
}

// ── Compiler ──────────────────────────────────────────────────────────────────

pub struct Compiler {
    /// Stack of function scopes: index 0 = top-level script, last = innermost function.
    scopes: Vec<FnScope>,
    /// The chunk being built for each function scope (parallel to `scopes`).
    chunks: Vec<Chunk>,
    /// Current source line (updated by SetLine stmts).
    current_line: usize,
}

impl Compiler {
    pub fn new() -> Self {
        let mut c = Compiler {
            scopes: vec![FnScope::new()],
            chunks: vec![Chunk::new()],
            current_line: 0,
        };
        // Register all global built-in names so they don't become undefined.
        // (They are resolved via GetGlobal at runtime; we just make sure the
        //  name table has them.)
        for name in BUILTINS {
            c.chunk().add_name(name);
        }
        c
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn chunk(&mut self) -> &mut Chunk {
        self.chunks.last_mut().unwrap()
    }

    fn scope(&mut self) -> &mut FnScope {
        self.scopes.last_mut().unwrap()
    }

    fn depth(&self) -> usize {
        self.scopes.len() - 1
    }

    fn emit(&mut self, op: Op) -> usize {
        let line = self.current_line;
        self.chunks.last_mut().unwrap().emit(op, line)
    }

    fn emit_jump<F: FnOnce(usize) -> Op>(&mut self, make: F) -> usize {
        let line = self.current_line;
        self.chunks.last_mut().unwrap().emit_jump(make, line)
    }

    fn patch(&mut self, idx: usize) {
        let target = self.chunks.last().unwrap().current_ip();
        self.chunks.last_mut().unwrap().patch_jump(idx, target);
    }

    fn add_constant(&mut self, v: VmValue) -> usize {
        self.chunks.last_mut().unwrap().add_constant(v)
    }

    fn add_name(&mut self, name: &str) -> usize {
        self.chunks.last_mut().unwrap().add_name(name)
    }

    // ── Variable resolution ───────────────────────────────────────────────────

    /// Resolve a variable name to the appropriate `Get*` opcode.
    fn resolve_get(&mut self, name: &str) -> Op {
        // Check for global/nonlocal declarations in current scope.
        if self.depth() > 0 {
            let (is_global, is_nonlocal) = {
                let s = self.scopes.last().unwrap();
                (s.globals_declared.contains(name), s.nonlocals_declared.contains(name))
            };
            if is_global {
                let idx = self.add_name(name);
                return Op::GetGlobal(idx);
            }
            if is_nonlocal {
                let uv_idx = self.resolve_upvalue(name, self.depth());
                if let Some(i) = uv_idx {
                    return Op::GetUpvalue(i);
                }
                let idx = self.add_name(name);
                return Op::GetGlobal(idx);
            }
        }

        // Local in the current function scope?
        if self.depth() > 0 {
            if let Some(slot) = self.scopes.last().unwrap().resolve_local(name) {
                return Op::GetLocal(slot);
            }
            // Upvalue from enclosing scope?
            if let Some(uv_idx) = self.resolve_upvalue(name, self.depth()) {
                return Op::GetUpvalue(uv_idx);
            }
        }

        // Fall back to global.
        let idx = self.add_name(name);
        Op::GetGlobal(idx)
    }

    /// Resolve a name to the appropriate `Set*` opcode.
    fn resolve_set(&mut self, name: &str) -> Op {
        if self.depth() > 0 {
            let (is_global, is_nonlocal) = {
                let s = self.scopes.last().unwrap();
                (s.globals_declared.contains(name), s.nonlocals_declared.contains(name))
            };
            if is_global {
                let idx = self.add_name(name);
                return Op::SetGlobal(idx);
            }
            if is_nonlocal {
                let uv_idx = self.resolve_upvalue(name, self.depth());
                if let Some(i) = uv_idx {
                    return Op::SetUpvalue(i);
                }
                let idx = self.add_name(name);
                return Op::SetGlobal(idx);
            }
        }

        if self.depth() > 0 {
            if let Some(slot) = self.scopes.last().unwrap().resolve_local(name) {
                return Op::SetLocal(slot);
            }
            // Check if it already exists as upvalue (for assignment via nonlocal-like semantics)
            if let Some(uv_idx) = self.resolve_upvalue(name, self.depth()) {
                return Op::SetUpvalue(uv_idx);
            }
            // New local in current function.
            let slot = self.scopes.last_mut().unwrap().add_local(name);
            return Op::SetLocal(slot);
        }

        // Top-level: global.
        let idx = self.add_name(name);
        Op::SetGlobal(idx)
    }

    /// Try to find `name` in enclosing scopes and create upvalue entries.
    /// Returns the upvalue index in the current scope, or None if not found
    /// in any enclosing function scope (globals are handled elsewhere).
    fn resolve_upvalue(&mut self, name: &str, depth: usize) -> Option<usize> {
        if depth == 0 { return None; }
        let parent = depth - 1;
        if parent == 0 {
            // Parent is global scope; don't create upvalues for globals.
            return None;
        }

        // Check if name is a local in the parent function scope.
        let parent_local = self.scopes[parent].resolve_local(name);
        if let Some(slot) = parent_local {
            // Mark as captured.
            if let Some(loc) = self.scopes[parent].locals.iter_mut().find(|l| l.slot == slot) {
                loc.is_captured = true;
            }
            let idx = self.scopes[depth].add_upvalue(name, UpvalueRef::Local(slot));
            return Some(idx);
        }

        // Recurse into grandparent.
        if let Some(parent_uv) = self.resolve_upvalue(name, parent) {
            let idx = self.scopes[depth].add_upvalue(name, UpvalueRef::Upvalue(parent_uv));
            return Some(idx);
        }

        None
    }

    // ── Public entry point ────────────────────────────────────────────────────

    pub fn compile_program(mut self, stmts: &[Stmt]) -> Result<Chunk, String> {
        for s in stmts {
            self.compile_stmt(s)?;
        }
        // Implicit nil return.
        self.emit(Op::Nil);
        self.emit(Op::Return);
        Ok(self.chunks.remove(0))
    }

    // ── Statement compilation ─────────────────────────────────────────────────

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::SetLine(n) => {
                self.current_line = *n;
                self.emit(Op::SetLine(*n));
            }

            Stmt::Expr(e) => {
                self.compile_expr(e)?;
                self.emit(Op::Pop);
            }

            Stmt::Assign { name, value } => {
                self.compile_expr(value)?;
                let op = self.resolve_set(name);
                self.emit(op);
            }

            Stmt::AugAssign { name, op, value } => {
                // Load current value.
                let get_op = self.resolve_get(name);
                self.emit(get_op);
                self.compile_expr(value)?;
                let bin = match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Pow => Op::Pow,
                    BinOp::FloorDiv => Op::FloorDiv,
                    BinOp::BitAnd => Op::BitAnd,
                    BinOp::BitOr => Op::BitOr,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::LShift => Op::LShift,
                    BinOp::RShift => Op::RShift,
                    _ => return Err(format!("unsupported augmented assignment op: {:?}", op)),
                };
                self.emit(bin);
                let set_op = self.resolve_set(name);
                self.emit(set_op);
            }

            Stmt::Unpack { names, value } => {
                self.compile_expr(value)?;
                self.emit(Op::Unpack(names.len()));
                // Unpack pushes values in order; assign right-to-left so the
                // first name gets the first value.
                for name in names.iter().rev() {
                    let op = self.resolve_set(name);
                    self.emit(op);
                }
            }

            Stmt::SetItem { object, index, value } => {
                self.compile_expr(object)?;
                self.compile_expr(index)?;
                self.compile_expr(value)?;
                self.emit(Op::SetItem);
            }

            Stmt::SetAttr { object, name, value } => {
                self.compile_expr(object)?;
                self.compile_expr(value)?;
                let idx = self.add_name(name);
                self.emit(Op::SetAttr(idx));
            }

            Stmt::Return(expr) => {
                match expr {
                    Some(e) => self.compile_expr(e)?,
                    None => { self.emit(Op::Nil); }
                }
                self.emit(Op::Return);
            }

            Stmt::Pass => {}

            Stmt::Break => {
                let patch_idx = self.emit_jump(Op::Jump);
                // Register for later patching.
                if let Some(loop_info) = self.scopes.last_mut().unwrap().loop_stack.last_mut() {
                    loop_info.break_patches.push(patch_idx);
                } else {
                    return Err("break outside loop".to_string());
                }
            }

            Stmt::Continue => {
                let target = self.scopes.last().unwrap().loop_stack.last()
                    .map(|l| l.continue_target)
                    .ok_or_else(|| "continue outside loop".to_string())?;
                self.emit(Op::Jump(target));
            }

            Stmt::If { condition, then_body, elif_clauses, else_body } => {
                // Compile: if cond { then } elif ... else { else }
                // We collect the patch points for the "jump past the whole if" at
                // the end of each branch.
                let mut end_patches: Vec<usize> = Vec::new();

                self.compile_expr(condition)?;
                let mut next_patch = self.emit_jump(Op::JumpIfFalse);

                // Then branch.
                self.emit(Op::Pop); // pop the condition
                for s in then_body { self.compile_stmt(s)?; }
                end_patches.push(self.emit_jump(Op::Jump));

                // Each elif.
                for (cond, body) in elif_clauses {
                    self.patch(next_patch);
                    self.emit(Op::Pop); // pop the previous condition
                    self.compile_expr(cond)?;
                    next_patch = self.emit_jump(Op::JumpIfFalse);
                    self.emit(Op::Pop);
                    for s in body { self.compile_stmt(s)?; }
                    end_patches.push(self.emit_jump(Op::Jump));
                }

                // Else branch.
                self.patch(next_patch);
                self.emit(Op::Pop); // pop condition
                if let Some(eb) = else_body {
                    for s in eb { self.compile_stmt(s)?; }
                }

                // Patch all end-of-branch jumps.
                for p in end_patches { self.patch(p); }
            }

            Stmt::While { condition, body } => {
                let loop_start = self.chunk().current_ip();
                self.scope().loop_stack.push(LoopInfo {
                    break_patches: Vec::new(),
                    continue_target: loop_start,
                });

                self.compile_expr(condition)?;
                let exit_patch = self.emit_jump(Op::JumpIfFalse);
                self.emit(Op::Pop); // pop truthy condition

                for s in body { self.compile_stmt(s)?; }

                self.emit(Op::Jump(loop_start));

                self.patch(exit_patch);
                self.emit(Op::Pop); // pop falsy condition

                let info = self.scopes.last_mut().unwrap().loop_stack.pop().unwrap();
                let after = self.chunk().current_ip();
                for p in info.break_patches { self.chunk().patch_jump(p, after); }
            }

            Stmt::For { var, iter, body } => {
                // Compile iterator expression.
                self.compile_expr(iter)?;
                self.emit(Op::GetIter);

                let loop_start = self.chunk().current_ip();
                self.scope().loop_stack.push(LoopInfo {
                    break_patches: Vec::new(),
                    continue_target: loop_start,
                });

                let exit_patch = self.emit_jump(Op::ForIter);
                // ForIter pushed the next value; assign it to the loop var.
                let set_op = self.resolve_set(var);
                self.emit(set_op);

                for s in body { self.compile_stmt(s)?; }

                self.emit(Op::Jump(loop_start));
                self.patch(exit_patch);

                let info = self.scopes.last_mut().unwrap().loop_stack.pop().unwrap();
                let after = self.chunk().current_ip();
                for p in info.break_patches { self.chunk().patch_jump(p, after); }
            }

            Stmt::FnDef { name, params, body } => {
                let proto = self.compile_fn(name, params, body)?;
                let upvalue_count = proto.upvalue_count;
                let upvalues: Vec<UpvalueRef> = {
                    // The current scope recorded upvalue refs when we compiled the nested fn.
                    // They were stored in the *child* scope; we need them after pop.
                    // compile_fn already popped the child scope; we captured them as
                    // UpvalueRef in the proto.  We re-read them from the FnProto's
                    // captured_refs that we return.
                    // Actually, we stored them on `self` during compile_fn. Let me restructure.
                    // → See compile_fn: it returns (proto, Vec<UpvalueRef>).
                    // This is handled inside compile_fn_inner below.
                    Vec::new() // placeholder, overridden by compile_fn_with_refs
                };
                let _ = upvalue_count;
                let _ = upvalues;
                // Use the two-return variant:
                let (proto, refs) = self.compile_fn_with_refs(name, params, body)?;
                let ci = self.add_constant(VmValue::Proto(Rc::new(proto)));
                self.emit(Op::MakeClosure(ci, refs));
                let op = self.resolve_set(name);
                self.emit(op);
            }

            Stmt::Class { name, parent, body } => {
                // Collect methods from body.
                let has_parent = parent.is_some();

                if let Some(pname) = parent {
                    let get = self.resolve_get(pname);
                    self.emit(get);
                }

                let name_idx = self.add_name(name);
                self.emit(Op::MakeClass(name_idx, has_parent));

                // The class is now on TOS. Compile methods and attach them.
                // We use DupTop + SetAttr for each method.
                for stmt in body {
                    match stmt {
                        Stmt::FnDef { name: method_name, params, body: method_body } => {
                            self.emit(Op::DupTop); // duplicate the class so we can SetAttr on it
                            let (proto, refs) = self.compile_fn_with_refs(method_name, params, method_body)?;
                            let ci = self.add_constant(VmValue::Proto(Rc::new(proto)));
                            self.emit(Op::MakeClosure(ci, refs));
                            let attr_idx = self.add_name(method_name);
                            self.emit(Op::SetAttr(attr_idx));
                        }
                        Stmt::Pass => {}
                        Stmt::SetLine(n) => {
                            self.current_line = *n;
                        }
                        _ => {
                            // Static class-body statements (not methods) — just run them
                            // (uncommon but technically valid).
                        }
                    }
                }

                // Store class in variable.
                let op = self.resolve_set(name);
                self.emit(op);
            }

            Stmt::Try { body, handlers, else_body, finally_body } => {
                self.compile_try(body, handlers, else_body.as_deref(), finally_body.as_deref())?;
            }

            Stmt::Raise(expr) => {
                match expr {
                    Some(e) => self.compile_expr(e)?,
                    None => { self.emit(Op::Nil); } // bare raise — VM will re-raise current
                }
                self.emit(Op::Raise);
            }

            Stmt::Assert { condition, message } => {
                self.compile_expr(condition)?;
                let pass = self.emit_jump(Op::JumpIfTrue);
                self.emit(Op::Pop);
                match message {
                    Some(m) => self.compile_expr(m)?,
                    None => {
                        let ci = self.add_constant(VmValue::Str("AssertionError".to_string()));
                        self.emit(Op::Constant(ci));
                    }
                }
                self.emit(Op::Raise);
                self.patch(pass);
                self.emit(Op::Pop);
            }

            Stmt::With { expr, as_name, body } => {
                self.compile_expr(expr)?;
                // Call __enter__ on the context manager.
                let enter_idx = self.add_name("__enter__");
                self.emit(Op::DupTop);
                self.emit(Op::GetAttr(enter_idx));
                self.emit(Op::Call(0, vec![]));
                if let Some(var) = as_name {
                    let op = self.resolve_set(var);
                    self.emit(op);
                } else {
                    self.emit(Op::Pop);
                }
                // TODO: proper __exit__ on exception; for now just compile body
                for s in body { self.compile_stmt(s)?; }
                // Call __exit__(None, None, None)
                let exit_idx = self.add_name("__exit__");
                self.emit(Op::GetAttr(exit_idx));
                self.emit(Op::Nil); self.emit(Op::Nil); self.emit(Op::Nil);
                self.emit(Op::Call(3, vec![]));
                self.emit(Op::Pop);
            }

            Stmt::Global(names) => {
                for n in names {
                    self.scopes.last_mut().unwrap().globals_declared.insert(n.clone());
                }
            }

            Stmt::Nonlocal(names) => {
                for n in names {
                    self.scopes.last_mut().unwrap().nonlocals_declared.insert(n.clone());
                }
            }

            Stmt::Import(path) => {
                // import "file.cool" — load and run, push module namespace
                let ci = self.add_constant(VmValue::Str(path.clone()));
                self.emit(Op::Constant(ci));
                let idx = self.add_name("__import_file__");
                self.emit(Op::GetGlobal(idx));
                self.emit(Op::Call(1, vec![]));
                self.emit(Op::Pop);
            }

            Stmt::ImportModule(module_name) => {
                // import math / os / sys / etc.
                let ci = self.add_constant(VmValue::Str(module_name.clone()));
                self.emit(Op::Constant(ci));
                let name_idx = self.add_name(module_name);
                // Set the module into a global with the module name
                // VM handles this specially via __import_module__ builtin.
                let bi = self.add_name("__import_module__");
                self.emit(Op::GetGlobal(bi));
                self.emit(Op::Constant(ci));
                self.emit(Op::Call(1, vec![]));
                self.emit(Op::SetGlobal(name_idx));
            }
        }
        Ok(())
    }

    // ── Try/except compilation ────────────────────────────────────────────────

    fn compile_try(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        else_body: Option<&[Stmt]>,
        finally_body: Option<&[Stmt]>,
    ) -> Result<(), String> {
        // SetupExcept(handler_ip) → try-body → PopExcept → else → jump(end)
        // handler_ip: exception on TOS
        //   for each handler: check type, bind name, body, jump(end)
        // end:
        // finally (if any)

        let setup_idx = self.emit_jump(Op::SetupExcept);

        // Try body.
        for s in body { self.compile_stmt(s)?; }

        self.emit(Op::PopExcept);

        // Else body (runs if no exception).
        if let Some(eb) = else_body {
            for s in eb { self.compile_stmt(s)?; }
        }

        let end_jump = self.emit_jump(Op::Jump);
        self.patch(setup_idx); // handler starts here; exception is on TOS

        // Handlers: exception value is on TOS at handler entry.
        // type_fail: Some(patch) = jump-if-false patch from typed handler; None = bare (always matches)
        let mut type_fail: Vec<Option<usize>> = Vec::new();
        let mut done_patches: Vec<usize> = Vec::new();

        for (i, handler) in handlers.iter().enumerate() {
            // Patch the previous handler's type-fail jump to here (next handler).
            if i > 0 {
                if let Some(Some(fail_patch)) = type_fail.last() {
                    let fp = *fail_patch;
                    self.patch(fp);
                    // Pop the false bool that JumpIfFalse left on stack.
                    self.emit(Op::Pop);
                }
                // If previous was bare except, it always matches; we'd never fall through.
            }

            if let Some(exc_type) = &handler.exc_type {
                // Stack: [..., exc]
                // ExcMatches peeks exc and pushes bool → [..., exc, bool]
                let type_idx = self.add_name(exc_type);
                self.emit(Op::ExcMatches(type_idx));
                let fail = self.emit_jump(Op::JumpIfFalse);
                self.emit(Op::Pop); // pop true bool, exc remains → [..., exc]
                type_fail.push(Some(fail));
            } else {
                // Bare except: always matches.
                type_fail.push(None);
            }

            // exc is on TOS; bind or discard.
            if let Some(bname) = &handler.as_name {
                let op = self.resolve_set(bname);
                self.emit(op);
            } else {
                self.emit(Op::Pop);
            }

            for s in &handler.body { self.compile_stmt(s)?; }

            // Jump past remaining handlers to end.
            done_patches.push(self.emit_jump(Op::Jump));
        }

        // If all typed handlers failed, re-raise.
        if let Some(Some(last_fail)) = type_fail.last() {
            let lf = *last_fail;
            self.patch(lf);
            self.emit(Op::Pop); // pop false bool
            // Stack: [..., exc]; raise it.
            self.emit(Op::Raise);
        }

        // `end` is here; patch the post-try-body jump.
        self.patch(end_jump);

        // Patch all handler-body done-jumps to end.
        for p in &done_patches { self.patch(*p); }

        // Finally body (always runs).
        if let Some(fb) = finally_body {
            for s in fb { self.compile_stmt(s)?; }
        }

        Ok(())
    }

    // ── Function compilation ──────────────────────────────────────────────────

    /// Compile a function definition, returning the FnProto and the upvalue
    /// capture descriptors needed to build the MakeClosure instruction.
    fn compile_fn_with_refs(
        &mut self,
        name: &str,
        params: &[Param],
        body: &[Stmt],
    ) -> Result<(FnProto, Vec<UpvalueRef>), String> {
        // Push a new scope + chunk.
        self.scopes.push(FnScope::new());
        self.chunks.push(Chunk::new());

        // Allocate local slots for parameters (in order).
        for p in params {
            if !p.is_vararg && !p.is_kwarg {
                self.scopes.last_mut().unwrap().add_local(&p.name);
            }
        }
        // *args slot
        let has_vararg = params.iter().any(|p| p.is_vararg);
        if has_vararg {
            let vname = params.iter().find(|p| p.is_vararg).unwrap().name.clone();
            self.scopes.last_mut().unwrap().add_local(&vname);
        }
        // **kwargs slot
        let has_kwarg = params.iter().any(|p| p.is_kwarg);
        if has_kwarg {
            let kname = params.iter().find(|p| p.is_kwarg).unwrap().name.clone();
            self.scopes.last_mut().unwrap().add_local(&kname);
        }

        for s in body { self.compile_stmt(s)?; }

        // Implicit return nil.
        self.emit(Op::Nil);
        self.emit(Op::Return);

        let scope = self.scopes.pop().unwrap();
        let chunk = self.chunks.pop().unwrap();

        let upvalue_refs: Vec<UpvalueRef> = scope.upvalues.iter().map(|u| u.capture.clone()).collect();
        let upvalue_count = upvalue_refs.len();
        let local_count = scope.next_slot;

        let proto = FnProto {
            name: name.to_string(),
            params: params.to_vec(),
            chunk,
            upvalue_count,
            local_count,
        };

        Ok((proto, upvalue_refs))
    }

    /// Unused wrapper kept for API compatibility.
    fn compile_fn(
        &mut self,
        name: &str,
        params: &[Param],
        body: &[Stmt],
    ) -> Result<FnProto, String> {
        let (proto, _) = self.compile_fn_with_refs(name, params, body)?;
        Ok(proto)
    }

    // ── Expression compilation ────────────────────────────────────────────────

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Int(n) => {
                let ci = self.add_constant(VmValue::Int(*n));
                self.emit(Op::Constant(ci));
            }
            Expr::Float(f) => {
                let ci = self.add_constant(VmValue::Float(*f));
                self.emit(Op::Constant(ci));
            }
            Expr::Str(s) => {
                let ci = self.add_constant(VmValue::Str(s.clone()));
                self.emit(Op::Constant(ci));
            }
            Expr::Bool(b) => {
                self.emit(if *b { Op::True } else { Op::False });
            }
            Expr::Nil => {
                self.emit(Op::Nil);
            }

            Expr::Ident(name) => {
                let op = self.resolve_get(name);
                self.emit(op);
            }

            Expr::BinOp { op, left, right } => {
                // Short-circuit for `and` / `or`.
                match op {
                    BinOp::And => {
                        self.compile_expr(left)?;
                        let j = self.emit_jump(Op::JumpIfFalse);
                        self.emit(Op::Pop);
                        self.compile_expr(right)?;
                        self.patch(j);
                        return Ok(());
                    }
                    BinOp::Or => {
                        self.compile_expr(left)?;
                        let j = self.emit_jump(Op::JumpIfTrue);
                        self.emit(Op::Pop);
                        self.compile_expr(right)?;
                        self.patch(j);
                        return Ok(());
                    }
                    _ => {}
                }

                self.compile_expr(left)?;
                self.compile_expr(right)?;
                let bin_op = match op {
                    BinOp::Add => Op::Add, BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul, BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod, BinOp::Pow => Op::Pow,
                    BinOp::FloorDiv => Op::FloorDiv,
                    BinOp::Eq => Op::Eq, BinOp::NotEq => Op::NotEq,
                    BinOp::Lt => Op::Lt, BinOp::LtEq => Op::LtEq,
                    BinOp::Gt => Op::Gt, BinOp::GtEq => Op::GtEq,
                    BinOp::In => Op::In, BinOp::NotIn => Op::NotIn,
                    BinOp::BitAnd => Op::BitAnd, BinOp::BitOr => Op::BitOr,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::LShift => Op::LShift, BinOp::RShift => Op::RShift,
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                self.emit(bin_op);
            }

            Expr::UnaryOp { op, expr } => {
                self.compile_expr(expr)?;
                let uop = match op {
                    UnaryOp::Neg => Op::Neg,
                    UnaryOp::Not => Op::Not,
                    UnaryOp::BitNot => Op::BitNot,
                };
                self.emit(uop);
            }

            Expr::Call { callee, args, kwargs } => {
                self.compile_expr(callee)?;
                let positional_count = args.len();
                for a in args { self.compile_expr(a)?; }
                let kwarg_names: Vec<String> = kwargs.iter().map(|(k, _)| k.clone()).collect();
                for (_, v) in kwargs { self.compile_expr(v)?; }
                self.emit(Op::Call(positional_count, kwarg_names));
            }

            Expr::Index { object, index } => {
                self.compile_expr(object)?;
                self.compile_expr(index)?;
                self.emit(Op::GetItem);
            }

            Expr::Slice { object, start, stop } => {
                self.compile_expr(object)?;
                match start { Some(e) => self.compile_expr(e)?, None => { self.emit(Op::Nil); } }
                match stop  { Some(e) => self.compile_expr(e)?, None => { self.emit(Op::Nil); } }
                self.emit(Op::GetSlice);
            }

            Expr::Attr { object, name } => {
                self.compile_expr(object)?;
                let idx = self.add_name(name);
                self.emit(Op::GetAttr(idx));
            }

            Expr::List(items) => {
                for item in items { self.compile_expr(item)?; }
                self.emit(Op::BuildList(items.len()));
            }

            Expr::Dict(pairs) => {
                for (k, v) in pairs {
                    self.compile_expr(k)?;
                    self.compile_expr(v)?;
                }
                self.emit(Op::BuildDict(pairs.len()));
            }

            Expr::Tuple(items) => {
                for item in items { self.compile_expr(item)?; }
                self.emit(Op::BuildTuple(items.len()));
            }

            Expr::FString(parts) => {
                let mut n = 0usize;
                for part in parts {
                    match part {
                        FStringPart::Literal(s) => {
                            let ci = self.add_constant(VmValue::Str(s.clone()));
                            self.emit(Op::Constant(ci));
                            n += 1;
                        }
                        FStringPart::Expr(e) => {
                            self.compile_expr(e)?;
                            // Convert to string (the VM's ConcatStr will do str() on non-strings).
                            n += 1;
                        }
                    }
                }
                if n != 1 {
                    self.emit(Op::ConcatStr(n));
                }
            }

            Expr::Lambda { params, body } => {
                let (proto, refs) = self.compile_fn_with_refs("<lambda>", params, &[Stmt::Return(Some(*body.clone()))])?;
                let ci = self.add_constant(VmValue::Proto(Rc::new(proto)));
                self.emit(Op::MakeClosure(ci, refs));
            }

            Expr::Ternary { condition, then_expr, else_expr } => {
                self.compile_expr(condition)?;
                let else_jump = self.emit_jump(Op::JumpIfFalse);
                self.emit(Op::Pop);
                self.compile_expr(then_expr)?;
                let end_jump = self.emit_jump(Op::Jump);
                self.patch(else_jump);
                self.emit(Op::Pop);
                self.compile_expr(else_expr)?;
                self.patch(end_jump);
            }

            Expr::ListComp { expr, var, iter, condition } => {
                // Compile as:
                //   result = []
                //   for var in iter: if condition: result.append(expr)
                // We implement this inline using a temporary local.
                let list_slot = self.scopes.last_mut().unwrap().add_local("<listcomp_result>");
                self.emit(Op::BuildList(0));
                let set_list = Op::SetLocal(list_slot);
                self.emit(set_list);

                // for var in iter:
                self.compile_expr(iter)?;
                self.emit(Op::GetIter);

                let loop_start = self.chunk().current_ip();
                self.scope().loop_stack.push(LoopInfo {
                    break_patches: Vec::new(),
                    continue_target: loop_start,
                });

                let exit_patch = self.emit_jump(Op::ForIter);
                let set_var = self.resolve_set(var);
                self.emit(set_var);

                // Optional condition.
                let mut cond_patch = None;
                if let Some(cond) = condition {
                    self.compile_expr(cond)?;
                    cond_patch = Some(self.emit_jump(Op::JumpIfFalse));
                    self.emit(Op::Pop);
                }

                // result.append(expr)
                self.emit(Op::GetLocal(list_slot));
                let append_idx = self.add_name("append");
                self.emit(Op::GetAttr(append_idx));
                self.compile_expr(expr)?;
                self.emit(Op::Call(1, vec![]));
                self.emit(Op::Pop);

                // Jump back to loop (skipping the false-condition path below).
                let true_continue = self.emit_jump(Op::Jump);

                if let Some(p) = cond_patch {
                    self.patch(p);          // false case jumps here
                    self.emit(Op::Pop);     // pop the peeked false condition bool
                }

                // Both true and false paths converge here.
                self.patch(true_continue);
                self.emit(Op::Jump(loop_start));
                self.patch(exit_patch);

                let info = self.scopes.last_mut().unwrap().loop_stack.pop().unwrap();
                let after = self.chunk().current_ip();
                for p in info.break_patches { self.chunk().patch_jump(p, after); }

                // Push result.
                self.emit(Op::GetLocal(list_slot));
                // Remove temp local (just decrement next_slot — won't affect execution).
                self.scopes.last_mut().unwrap().next_slot -= 1;
                self.scopes.last_mut().unwrap().locals.pop();
            }
        }
        Ok(())
    }
}

// ── Built-in name list ────────────────────────────────────────────────────────

const BUILTINS: &[&str] = &[
    "print", "len", "range", "str", "int", "float", "bool", "type",
    "input", "repr", "exit", "open", "isinstance", "hasattr", "getattr",
    "list", "tuple", "dict", "set", "sorted", "reversed", "enumerate",
    "zip", "abs", "min", "max", "sum", "map", "filter",
    "set_completions", "eval", "append", "pop", "keys", "values", "items",
    "runfile", "super",
    "__import_file__", "__import_module__", "__exc_matches__",
];

// ── Public API ────────────────────────────────────────────────────────────────

pub fn compile(program: &crate::ast::Program) -> Result<Chunk, String> {
    Compiler::new().compile_program(program)
}
