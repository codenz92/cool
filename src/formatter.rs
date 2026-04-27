use crate::ast::{BinOp, ExceptHandler, Expr, FStringPart, Param, Stmt, UnaryOp, Visibility};
use crate::lexer::Lexer;
use crate::parser::Parser;

#[derive(Clone)]
struct CommentLine {
    line: usize,
    indent_width: usize,
    text: String,
    inline: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    Lowest = 0,
    Lambda = 1,
    Ternary = 2,
    Or = 3,
    And = 4,
    Compare = 5,
    BitOr = 6,
    BitXor = 7,
    BitAnd = 8,
    Shift = 9,
    Add = 10,
    Mul = 11,
    Unary = 12,
    Pow = 13,
    Postfix = 14,
    Atom = 15,
}

struct Formatter {
    out: String,
    comments: Vec<CommentLine>,
    next_comment: usize,
}

#[derive(Clone, Copy)]
struct LocatedStmt<'a> {
    line: usize,
    stmt: &'a Stmt,
}

pub fn format_source(source: &str) -> Result<String, String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    let comments = extract_comments(source);
    let mut formatter = Formatter {
        out: String::new(),
        comments,
        next_comment: 0,
    };
    formatter.write_stmt_list(&collect_located(&program), 0)?;
    formatter.emit_remaining_comments(0);
    if !formatter.out.ends_with('\n') {
        formatter.out.push('\n');
    }
    Ok(formatter.out)
}

fn collect_located<'a>(program: &'a [Stmt]) -> Vec<LocatedStmt<'a>> {
    let mut out = Vec::new();
    let mut current_line = 1usize;
    for stmt in program {
        match stmt {
            Stmt::SetLine(line) => current_line = *line,
            other => out.push(LocatedStmt {
                line: current_line,
                stmt: other,
            }),
        }
    }
    out
}

fn collect_located_block<'a>(stmts: &'a [Stmt]) -> Vec<LocatedStmt<'a>> {
    collect_located(stmts)
}

impl Formatter {
    fn write_stmt_list(&mut self, stmts: &[LocatedStmt<'_>], indent: usize) -> Result<(), String> {
        for (idx, located) in stmts.iter().enumerate() {
            self.emit_comments_before(located.line, indent);
            self.write_stmt(located.stmt, located.line, indent, None)?;
            if idx + 1 < stmts.len() {
                let current = stmt_needs_spacing(located.stmt);
                let next = stmt_needs_spacing(stmts[idx + 1].stmt);
                if current || next {
                    self.out.push('\n');
                }
            }
        }
        self.emit_trailing_block_comments(indent);
        Ok(())
    }

    fn write_stmt(&mut self, stmt: &Stmt, line: usize, indent: usize, prefix: Option<&str>) -> Result<(), String> {
        match stmt {
            Stmt::Visibility { visibility, stmt } => {
                let prefix = match visibility {
                    Visibility::Public => "public ",
                    Visibility::Private => "private ",
                };
                self.write_stmt(stmt, line, indent, Some(prefix))
            }
            Stmt::Expr(expr) => self.write_line(
                indent,
                &format!("{}{}", prefix.unwrap_or(""), self.expr(expr, Prec::Lowest)),
                line,
            ),
            Stmt::Assign { name, value } => self.write_line(
                indent,
                &format!("{}{} = {}", prefix.unwrap_or(""), name, self.expr(value, Prec::Lowest)),
                line,
            ),
            Stmt::VarDecl {
                name,
                type_name,
                value,
                is_const,
            } => {
                let mut text = String::new();
                text.push_str(prefix.unwrap_or(""));
                if *is_const {
                    text.push_str("const ");
                }
                text.push_str(name);
                if let Some(type_name) = type_name {
                    text.push_str(": ");
                    text.push_str(type_name);
                }
                text.push_str(" = ");
                text.push_str(&self.expr(value, Prec::Lowest));
                self.write_line(indent, &text, line)
            }
            Stmt::SetItem { object, index, value } => self.write_line(
                indent,
                &format!(
                    "{}{}[{}] = {}",
                    prefix.unwrap_or(""),
                    self.expr(object, Prec::Postfix),
                    self.expr(index, Prec::Lowest),
                    self.expr(value, Prec::Lowest)
                ),
                line,
            ),
            Stmt::SetAttr { object, name, value } => self.write_line(
                indent,
                &format!(
                    "{}{}.{} = {}",
                    prefix.unwrap_or(""),
                    self.expr(object, Prec::Postfix),
                    name,
                    self.expr(value, Prec::Lowest)
                ),
                line,
            ),
            Stmt::AugAssign { name, op, value } => self.write_line(
                indent,
                &format!(
                    "{}{} {}= {}",
                    prefix.unwrap_or(""),
                    name,
                    aug_assign_op(*op)?,
                    self.expr(value, Prec::Lowest)
                ),
                line,
            ),
            Stmt::Unpack { names, value } => self.write_line(
                indent,
                &format!(
                    "{}{} = {}",
                    prefix.unwrap_or(""),
                    names.join(", "),
                    self.expr(value, Prec::Lowest)
                ),
                line,
            ),
            Stmt::UnpackTargets { targets, value } => {
                let targets = targets
                    .iter()
                    .map(|target| self.expr(target, Prec::Lowest))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.write_line(
                    indent,
                    &format!(
                        "{}{} = {}",
                        prefix.unwrap_or(""),
                        targets,
                        self.expr(value, Prec::Lowest)
                    ),
                    line,
                )
            }
            Stmt::Return(value) => {
                let text = match value {
                    Some(expr) => format!("{}return {}", prefix.unwrap_or(""), self.expr(expr, Prec::Lowest)),
                    None => format!("{}return", prefix.unwrap_or("")),
                };
                self.write_line(indent, &text, line)
            }
            Stmt::Break => self.write_line(indent, &format!("{}break", prefix.unwrap_or("")), line),
            Stmt::Continue => self.write_line(indent, &format!("{}continue", prefix.unwrap_or("")), line),
            Stmt::Pass => self.write_line(indent, &format!("{}pass", prefix.unwrap_or("")), line),
            Stmt::If {
                condition,
                then_body,
                elif_clauses,
                else_body,
            } => {
                self.write_line(
                    indent,
                    &format!("{}if {}:", prefix.unwrap_or(""), self.expr(condition, Prec::Lowest)),
                    line,
                )?;
                self.write_block(then_body, indent + 4)?;
                for (elif_cond, elif_body) in elif_clauses {
                    self.write_plain_line(indent, &format!("elif {}:", self.expr(elif_cond, Prec::Lowest)));
                    self.write_block(elif_body, indent + 4)?;
                }
                if let Some(else_body) = else_body {
                    self.write_plain_line(indent, "else:");
                    self.write_block(else_body, indent + 4)?;
                }
                Ok(())
            }
            Stmt::While { condition, body } => {
                self.write_line(
                    indent,
                    &format!("{}while {}:", prefix.unwrap_or(""), self.expr(condition, Prec::Lowest)),
                    line,
                )?;
                self.write_block(body, indent + 4)
            }
            Stmt::For { var, iter, body } => {
                self.write_line(
                    indent,
                    &format!(
                        "{}for {} in {}:",
                        prefix.unwrap_or(""),
                        var,
                        self.expr(iter, Prec::Lowest)
                    ),
                    line,
                )?;
                self.write_block(body, indent + 4)
            }
            Stmt::FnDef {
                name,
                params,
                return_type,
                section,
                entry,
                body,
            } => {
                let mut text = format!("{}def {}({})", prefix.unwrap_or(""), name, format_params(params));
                if let Some(return_type) = return_type {
                    text.push_str(" -> ");
                    text.push_str(return_type);
                }
                text.push(':');
                self.write_line(indent, &text, line)?;
                let metadata_indent = indent + 4;
                if let Some(section) = section {
                    self.write_plain_line(metadata_indent, &format!("section: {}", quote_metadata(section)));
                }
                if let Some(entry) = entry {
                    self.write_plain_line(metadata_indent, &format!("entry: {}", quote_metadata(entry)));
                }
                self.write_block(body, metadata_indent)
            }
            Stmt::ExternFn {
                name,
                params,
                return_type,
                symbol,
                callconv,
                section,
            } => {
                let mut text = format!(
                    "{}extern def {}({}) -> {}",
                    prefix.unwrap_or(""),
                    name,
                    format_extern_params(params),
                    return_type
                );
                if symbol.is_some() || callconv.is_some() || section.is_some() {
                    text.push(':');
                    self.write_line(indent, &text, line)?;
                    if let Some(symbol) = symbol {
                        self.write_plain_line(indent + 4, &format!("symbol: {}", quote_metadata(symbol)));
                    }
                    if let Some(callconv) = callconv {
                        self.write_plain_line(indent + 4, &format!("cc: {}", quote_metadata(callconv)));
                    }
                    if let Some(section) = section {
                        self.write_plain_line(indent + 4, &format!("section: {}", quote_metadata(section)));
                    }
                    Ok(())
                } else {
                    self.write_line(indent, &text, line)
                }
            }
            Stmt::Data {
                name,
                type_name,
                value,
                section,
            } => {
                let mut text = format!(
                    "{}data {}: {} = {}",
                    prefix.unwrap_or(""),
                    name,
                    type_name,
                    self.expr(value, Prec::Lowest)
                );
                if let Some(section) = section {
                    text.push(':');
                    self.write_line(indent, &text, line)?;
                    self.write_plain_line(indent + 4, &format!("section: {}", quote_metadata(section)));
                    Ok(())
                } else {
                    self.write_line(indent, &text, line)
                }
            }
            Stmt::Class { name, parent, body } => {
                let mut text = format!("{}class {}", prefix.unwrap_or(""), name);
                if let Some(parent) = parent {
                    text.push('(');
                    text.push_str(parent);
                    text.push(')');
                }
                text.push(':');
                self.write_line(indent, &text, line)?;
                self.write_block(body, indent + 4)
            }
            Stmt::Struct {
                name,
                fields,
                is_packed,
            } => {
                let keyword = if *is_packed { "packed struct" } else { "struct" };
                self.write_line(indent, &format!("{}{} {}:", prefix.unwrap_or(""), keyword, name), line)?;
                if fields.is_empty() {
                    self.write_plain_line(indent + 4, "pass");
                } else {
                    for (field_name, type_name) in fields {
                        self.write_plain_line(indent + 4, &format!("{field_name}: {type_name}"));
                    }
                }
                Ok(())
            }
            Stmt::Union { name, fields } => {
                self.write_line(indent, &format!("{}union {}:", prefix.unwrap_or(""), name), line)?;
                if fields.is_empty() {
                    self.write_plain_line(indent + 4, "pass");
                } else {
                    for (field_name, type_name) in fields {
                        self.write_plain_line(indent + 4, &format!("{field_name}: {type_name}"));
                    }
                }
                Ok(())
            }
            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                self.write_line(indent, &format!("{}try:", prefix.unwrap_or("")), line)?;
                self.write_block(body, indent + 4)?;
                for ExceptHandler {
                    exc_type,
                    as_name,
                    body,
                } in handlers
                {
                    let mut text = "except".to_string();
                    if let Some(exc_type) = exc_type {
                        text.push(' ');
                        text.push_str(exc_type);
                    }
                    if let Some(as_name) = as_name {
                        text.push_str(" as ");
                        text.push_str(as_name);
                    }
                    text.push(':');
                    self.write_plain_line(indent, &text);
                    self.write_block(body, indent + 4)?;
                }
                if let Some(else_body) = else_body {
                    self.write_plain_line(indent, "else:");
                    self.write_block(else_body, indent + 4)?;
                }
                if let Some(finally_body) = finally_body {
                    self.write_plain_line(indent, "finally:");
                    self.write_block(finally_body, indent + 4)?;
                }
                Ok(())
            }
            Stmt::Raise(value) => {
                let text = match value {
                    Some(expr) => format!("{}raise {}", prefix.unwrap_or(""), self.expr(expr, Prec::Lowest)),
                    None => format!("{}raise", prefix.unwrap_or("")),
                };
                self.write_line(indent, &text, line)
            }
            Stmt::Import(path) => self.write_line(
                indent,
                &format!("{}import {}", prefix.unwrap_or(""), quote_string(path)),
                line,
            ),
            Stmt::ImportModule(name) => {
                self.write_line(indent, &format!("{}import {}", prefix.unwrap_or(""), name), line)
            }
            Stmt::Assert { condition, message } => {
                let mut text = format!("{}assert {}", prefix.unwrap_or(""), self.expr(condition, Prec::Lowest));
                if let Some(message) = message {
                    text.push_str(", ");
                    text.push_str(&self.expr(message, Prec::Lowest));
                }
                self.write_line(indent, &text, line)
            }
            Stmt::With { expr, as_name, body } => {
                let mut text = format!("{}with {}", prefix.unwrap_or(""), self.expr(expr, Prec::Lowest));
                if let Some(as_name) = as_name {
                    text.push_str(" as ");
                    text.push_str(as_name);
                }
                text.push(':');
                self.write_line(indent, &text, line)?;
                self.write_block(body, indent + 4)
            }
            Stmt::Global(names) => self.write_line(
                indent,
                &format!("{}global {}", prefix.unwrap_or(""), names.join(", ")),
                line,
            ),
            Stmt::Nonlocal(names) => self.write_line(
                indent,
                &format!("{}nonlocal {}", prefix.unwrap_or(""), names.join(", ")),
                line,
            ),
            Stmt::SetLine(_) => Ok(()),
        }
    }

    fn write_block(&mut self, body: &[Stmt], indent: usize) -> Result<(), String> {
        let located = collect_located_block(body);
        if located.is_empty() {
            self.write_plain_line(indent, "pass");
            return Ok(());
        }
        self.write_stmt_list(&located, indent)
    }

    fn write_line(&mut self, indent: usize, text: &str, source_line: usize) -> Result<(), String> {
        self.write_plain_line(indent, text);
        if let Some(comment) = self.take_inline_comment(source_line) {
            if self.out.ends_with('\n') {
                self.out.pop();
            }
            self.out.push_str("  ");
            self.out.push_str(&comment.text);
            self.out.push('\n');
        }
        Ok(())
    }

    fn write_plain_line(&mut self, indent: usize, text: &str) {
        self.out.push_str(&" ".repeat(indent));
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn emit_comments_before(&mut self, line: usize, indent: usize) {
        while let Some(comment) = self.comments.get(self.next_comment) {
            if comment.inline || comment.line >= line {
                break;
            }
            let text = comment.text.clone();
            self.write_plain_line(indent, &text);
            self.next_comment += 1;
        }
    }

    fn emit_trailing_block_comments(&mut self, indent: usize) {
        while let Some(comment) = self.comments.get(self.next_comment) {
            if comment.inline || comment.indent_width < indent {
                break;
            }
            let text = comment.text.clone();
            self.write_plain_line(indent, &text);
            self.next_comment += 1;
        }
    }

    fn emit_remaining_comments(&mut self, indent: usize) {
        while let Some(comment) = self.comments.get(self.next_comment) {
            if !comment.inline {
                let text = comment.text.clone();
                self.write_plain_line(indent, &text);
            }
            self.next_comment += 1;
        }
    }

    fn take_inline_comment(&mut self, line: usize) -> Option<CommentLine> {
        if let Some(comment) = self.comments.get(self.next_comment) {
            if comment.inline && comment.line == line {
                self.next_comment += 1;
                return Some(comment.clone());
            }
        }
        None
    }

    fn expr(&self, expr: &Expr, parent: Prec) -> String {
        let prec = expr_prec(expr);
        let text = match expr {
            Expr::Int(value) => value.to_string(),
            Expr::Float(value) => {
                let mut text = value.to_string();
                if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                    text.push_str(".0");
                }
                text
            }
            Expr::Str(value) => quote_string(value),
            Expr::Bool(value) => value.to_string(),
            Expr::Nil => "nil".to_string(),
            Expr::Ident(name) => name.clone(),
            Expr::UnaryOp { op, expr } => format!("{}{}", unary_op_text(*op), self.expr(expr, Prec::Unary)),
            Expr::BinOp { op, left, right } => {
                let current = binop_prec(*op);
                let left = self.expr(left, current);
                let right_parent = if matches!(op, BinOp::Pow) { Prec::Unary } else { current };
                let right = self.expr(right, right_parent);
                format!("{left} {} {right}", binop_text(*op))
            }
            Expr::Call { callee, args, kwargs } => {
                let mut items = args.iter().map(|arg| self.expr(arg, Prec::Lowest)).collect::<Vec<_>>();
                items.extend(
                    kwargs
                        .iter()
                        .map(|(name, value)| format!("{name}={}", self.expr(value, Prec::Lowest))),
                );
                format!("{}({})", self.expr(callee, Prec::Postfix), items.join(", "))
            }
            Expr::Index { object, index } => {
                format!(
                    "{}[{}]",
                    self.expr(object, Prec::Postfix),
                    self.expr(index, Prec::Lowest)
                )
            }
            Expr::Slice { object, start, stop } => {
                let start = start
                    .as_deref()
                    .map(|expr| self.expr(expr, Prec::Lowest))
                    .unwrap_or_default();
                let stop = stop
                    .as_deref()
                    .map(|expr| self.expr(expr, Prec::Lowest))
                    .unwrap_or_default();
                format!("{}[{start}:{stop}]", self.expr(object, Prec::Postfix))
            }
            Expr::Attr { object, name } => format!("{}.{}", self.expr(object, Prec::Postfix), name),
            Expr::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|item| self.expr(item, Prec::Lowest))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Dict(items) => format!(
                "{{{}}}",
                items
                    .iter()
                    .map(|(key, value)| format!("{}: {}", self.expr(key, Prec::Lowest), self.expr(value, Prec::Lowest)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Tuple(items) => {
                let inner = items
                    .iter()
                    .map(|item| self.expr(item, Prec::Lowest))
                    .collect::<Vec<_>>()
                    .join(", ");
                if items.len() == 1 {
                    format!("({inner},)")
                } else {
                    format!("({inner})")
                }
            }
            Expr::FString(parts) => format_fstring(parts, self),
            Expr::Lambda { params, body } => {
                format!("lambda {}: {}", format_params(params), self.expr(body, Prec::Lambda))
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => format!(
                "{} if {} else {}",
                self.expr(then_expr, Prec::Ternary),
                self.expr(condition, Prec::Or),
                self.expr(else_expr, Prec::Ternary)
            ),
            Expr::ListComp {
                expr,
                var,
                iter,
                condition,
            } => {
                let mut text = format!(
                    "[{} for {} in {}",
                    self.expr(expr, Prec::Lowest),
                    var,
                    self.expr(iter, Prec::Lowest)
                );
                if let Some(condition) = condition {
                    text.push_str(" if ");
                    text.push_str(&self.expr(condition, Prec::Lowest));
                }
                text.push(']');
                text
            }
        };

        if prec < parent {
            format!("({text})")
        } else {
            text
        }
    }
}

fn stmt_needs_spacing(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::FnDef { .. }
            | Stmt::ExternFn { .. }
            | Stmt::Data { .. }
            | Stmt::Class { .. }
            | Stmt::Struct { .. }
            | Stmt::Union { .. }
            | Stmt::Try { .. }
            | Stmt::If { .. }
            | Stmt::While { .. }
            | Stmt::For { .. }
            | Stmt::Visibility { .. }
    )
}

fn format_params(params: &[Param]) -> String {
    params.iter().map(format_param).collect::<Vec<_>>().join(", ")
}

fn format_param(param: &Param) -> String {
    let mut text = String::new();
    if param.is_kwarg {
        text.push_str("**");
    } else if param.is_vararg {
        text.push('*');
    }
    text.push_str(&param.name);
    if let Some(type_name) = &param.type_name {
        text.push_str(": ");
        text.push_str(type_name);
    }
    if let Some(default) = &param.default {
        let formatter = Formatter {
            out: String::new(),
            comments: Vec::new(),
            next_comment: 0,
        };
        text.push_str(" = ");
        text.push_str(&formatter.expr(default, Prec::Lowest));
    }
    text
}

fn format_extern_params(params: &[crate::ast::ExternParam]) -> String {
    params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.type_name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn expr_prec(expr: &Expr) -> Prec {
    match expr {
        Expr::Lambda { .. } => Prec::Lambda,
        Expr::Ternary { .. } => Prec::Ternary,
        Expr::BinOp { op, .. } => binop_prec(*op),
        Expr::UnaryOp { .. } => Prec::Unary,
        Expr::Call { .. } | Expr::Index { .. } | Expr::Slice { .. } | Expr::Attr { .. } => Prec::Postfix,
        _ => Prec::Atom,
    }
}

fn binop_prec(op: BinOp) -> Prec {
    match op {
        BinOp::Or => Prec::Or,
        BinOp::And => Prec::And,
        BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq | BinOp::In | BinOp::NotIn => {
            Prec::Compare
        }
        BinOp::BitOr => Prec::BitOr,
        BinOp::BitXor => Prec::BitXor,
        BinOp::BitAnd => Prec::BitAnd,
        BinOp::LShift | BinOp::RShift => Prec::Shift,
        BinOp::Add | BinOp::Sub => Prec::Add,
        BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::FloorDiv => Prec::Mul,
        BinOp::Pow => Prec::Pow,
    }
}

fn binop_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::FloorDiv => "//",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::LtEq => "<=",
        BinOp::Gt => ">",
        BinOp::GtEq => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::In => "in",
        BinOp::NotIn => "not in",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::LShift => "<<",
        BinOp::RShift => ">>",
    }
}

fn aug_assign_op(op: BinOp) -> Result<&'static str, String> {
    match op {
        BinOp::Add => Ok("+"),
        BinOp::Sub => Ok("-"),
        BinOp::Mul => Ok("*"),
        BinOp::Div => Ok("/"),
        other => Err(format!(
            "unsupported augmented assignment operator in formatter: {other:?}"
        )),
    }
}

fn unary_op_text(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "not ",
        UnaryOp::BitNot => "~",
    }
}

fn quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn quote_metadata(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
    {
        value.to_string()
    } else {
        quote_string(value)
    }
}

fn format_fstring(parts: &[FStringPart], formatter: &Formatter) -> String {
    let mut out = String::from("f\"");
    for part in parts {
        match part {
            FStringPart::Literal(text) => {
                for ch in text.chars() {
                    match ch {
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        '{' => out.push_str("{{"),
                        '}' => out.push_str("}}"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(ch),
                    }
                }
            }
            FStringPart::Expr(expr) => {
                out.push('{');
                out.push_str(&formatter.expr(expr, Prec::Lowest));
                out.push('}');
            }
        }
    }
    out.push('"');
    out
}

fn extract_comments(source: &str) -> Vec<CommentLine> {
    #[derive(Clone, Copy)]
    enum StringState {
        None,
        Single,
        Double,
        TripleSingle,
        TripleDouble,
    }

    let mut state = StringState::None;
    let mut comments = Vec::new();
    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let indent_width = raw_line.chars().take_while(|ch| *ch == ' ' || *ch == '\t').count();
        let chars: Vec<char> = raw_line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let ch = chars[i];
            let next = chars.get(i + 1).copied();
            let third = chars.get(i + 2).copied();
            match state {
                StringState::None => match ch {
                    '#' => {
                        let prefix: String = chars[..i].iter().collect();
                        comments.push(CommentLine {
                            line: line_no,
                            indent_width,
                            text: raw_line[i..].trim_end().to_string(),
                            inline: !prefix.trim().is_empty(),
                        });
                        break;
                    }
                    '\'' if next == Some('\'') && third == Some('\'') => {
                        state = StringState::TripleSingle;
                        i += 3;
                        continue;
                    }
                    '"' if next == Some('"') && third == Some('"') => {
                        state = StringState::TripleDouble;
                        i += 3;
                        continue;
                    }
                    '\'' => state = StringState::Single,
                    '"' => state = StringState::Double,
                    _ => {}
                },
                StringState::Single => {
                    if ch == '\\' {
                        i += 2;
                        continue;
                    }
                    if ch == '\'' {
                        state = StringState::None;
                    }
                }
                StringState::Double => {
                    if ch == '\\' {
                        i += 2;
                        continue;
                    }
                    if ch == '"' {
                        state = StringState::None;
                    }
                }
                StringState::TripleSingle => {
                    if ch == '\'' && next == Some('\'') && third == Some('\'') {
                        state = StringState::None;
                        i += 3;
                        continue;
                    }
                }
                StringState::TripleDouble => {
                    if ch == '"' && next == Some('"') && third == Some('"') {
                        state = StringState::None;
                        i += 3;
                        continue;
                    }
                }
            }
            i += 1;
        }

        if matches!(state, StringState::Single | StringState::Double) {
            state = StringState::None;
        }
    }
    comments
}
