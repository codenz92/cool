/// Recursive-descent parser for Cool.
use crate::ast::*;
use crate::lexer::{Tok, Token};

pub struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Tok>) -> Self {
        Parser { tokens, pos: 0 }
    }

    // ── Token navigation ──────────────────────────────────────────────────

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn line(&self) -> usize {
        self.tokens[self.pos].span.line
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].token.clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, token: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(token)
    }

    fn eat(&mut self, token: &Token) -> Result<(), String> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(token) {
            self.advance();
            Ok(())
        } else {
            Err(format!(
                "line {}: expected {:?}, got {:?}",
                self.line(),
                token,
                self.peek()
            ))
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&Token::Newline) {
            self.advance();
        }
    }

    /// Skip newlines, indents, and dedents — safe inside collection literals
    fn skip_collection_ws(&mut self) {
        while matches!(self.peek(), Token::Newline | Token::Indent | Token::Dedent) {
            self.advance();
        }
    }

    fn token_at(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx].token
    }

    // ── Program / block ───────────────────────────────────────────────────

    /// Returns true if the parser has consumed all meaningful tokens (only Eof/Newline remain).
    pub fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof | Token::Newline)
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&Token::Eof) {
            stmts.push(Stmt::SetLine(self.line()));
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.eat(&Token::Indent)?;
        self.skip_newlines();
        let mut stmts = Vec::new();
        while !self.check(&Token::Dedent) && !self.check(&Token::Eof) {
            stmts.push(Stmt::SetLine(self.line()));
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        self.eat(&Token::Dedent)?;
        Ok(stmts)
    }

    // ── Statements ────────────────────────────────────────────────────────

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek().clone() {
            Token::Def => self.parse_fn_def(),
            Token::Class => self.parse_class(),
            Token::Struct => self.parse_struct(false),
            Token::Packed => {
                self.advance();
                self.eat(&Token::Struct)?;
                self.parse_struct_body(true)
            }
            Token::Union => self.parse_union(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::Return => self.parse_return(),
            Token::Break => {
                self.advance();
                self.eat_newline();
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                self.eat_newline();
                Ok(Stmt::Continue)
            }
            Token::Pass => {
                self.advance();
                self.eat_newline();
                Ok(Stmt::Pass)
            }
            Token::Import => self.parse_import(),
            Token::Try => self.parse_try(),
            Token::Raise => self.parse_raise(),
            Token::Assert => self.parse_assert(),
            Token::With => self.parse_with(),
            Token::Global => self.parse_global(),
            Token::Nonlocal => self.parse_nonlocal(),
            _ => self.parse_assign_or_expr(),
        }
    }

    fn eat_newline(&mut self) {
        if self.check(&Token::Newline) {
            self.advance();
        }
    }

    fn parse_fn_def(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Def)?;
        let name = self.expect_ident()?;
        self.eat(&Token::LParen)?;
        let params = self.parse_param_list()?;
        self.eat(&Token::RParen)?;
        self.eat(&Token::Colon)?;
        self.eat_newline();
        let body = self.parse_block()?;
        Ok(Stmt::FnDef { name, params, body })
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>, String> {
        let mut params = Vec::new();
        if self.check(&Token::RParen) {
            return Ok(params);
        }
        params.push(self.parse_param()?);
        while self.check(&Token::Comma) {
            self.advance();
            if self.check(&Token::RParen) {
                break;
            }
            params.push(self.parse_param()?);
        }
        Ok(params)
    }

    fn parse_param(&mut self) -> Result<Param, String> {
        // **kwargs
        if self.check(&Token::StarStar) {
            self.advance();
            let name = self.expect_ident()?;
            return Ok(Param {
                name,
                default: None,
                is_vararg: false,
                is_kwarg: true,
            });
        }
        // *args
        if self.check(&Token::Star) {
            self.advance();
            let name = self.expect_ident()?;
            return Ok(Param {
                name,
                default: None,
                is_vararg: true,
                is_kwarg: false,
            });
        }
        let name = self.expect_ident()?;
        if self.check(&Token::Eq) {
            self.advance();
            let default = self.parse_expr()?;
            Ok(Param {
                name,
                default: Some(default),
                is_vararg: false,
                is_kwarg: false,
            })
        } else {
            Ok(Param {
                name,
                default: None,
                is_vararg: false,
                is_kwarg: false,
            })
        }
    }

    fn parse_class(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Class)?;
        let name = self.expect_ident()?;
        let parent = if self.check(&Token::LParen) {
            self.advance();
            let p = self.expect_ident()?;
            self.eat(&Token::RParen)?;
            Some(p)
        } else {
            None
        };
        self.eat(&Token::Colon)?;
        self.eat_newline();
        let body = self.parse_block()?;
        Ok(Stmt::Class { name, parent, body })
    }

    fn parse_struct(&mut self, is_packed: bool) -> Result<Stmt, String> {
        self.eat(&Token::Struct)?;
        self.parse_struct_body(is_packed)
    }

    fn parse_struct_body(&mut self, is_packed: bool) -> Result<Stmt, String> {
        let name = self.expect_ident()?;
        self.eat(&Token::Colon)?;
        self.eat_newline();
        self.eat(&Token::Indent)?;
        let mut fields = Vec::new();
        while !self.check(&Token::Dedent) && !self.check(&Token::Eof) {
            self.skip_newlines();
            if self.check(&Token::Dedent) || self.check(&Token::Eof) {
                break;
            }
            if self.check(&Token::Pass) {
                self.advance();
                self.eat_newline();
                continue;
            }
            let field_name = self.expect_ident()?;
            self.eat(&Token::Colon)?;
            let type_name = self.expect_ident()?;
            self.eat_newline();
            fields.push((field_name, type_name));
        }
        self.eat(&Token::Dedent)?;
        Ok(Stmt::Struct { name, fields, is_packed })
    }

    fn parse_union(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Union)?;
        let name = self.expect_ident()?;
        self.eat(&Token::Colon)?;
        self.eat_newline();
        self.eat(&Token::Indent)?;
        let mut fields = Vec::new();
        while !self.check(&Token::Dedent) && !self.check(&Token::Eof) {
            self.skip_newlines();
            if self.check(&Token::Dedent) || self.check(&Token::Eof) {
                break;
            }
            if self.check(&Token::Pass) {
                self.advance();
                self.eat_newline();
                continue;
            }
            let field_name = self.expect_ident()?;
            self.eat(&Token::Colon)?;
            let type_name = self.expect_ident()?;
            self.eat_newline();
            fields.push((field_name, type_name));
        }
        self.eat(&Token::Dedent)?;
        Ok(Stmt::Union { name, fields })
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::If)?;
        let condition = self.parse_expr()?;
        self.eat(&Token::Colon)?;
        self.eat_newline();
        let then_body = self.parse_block()?;

        let mut elif_clauses = Vec::new();
        let mut else_body = None;

        loop {
            self.skip_newlines();
            if self.check(&Token::Elif) {
                self.advance();
                let cond = self.parse_expr()?;
                self.eat(&Token::Colon)?;
                self.eat_newline();
                let body = self.parse_block()?;
                elif_clauses.push((cond, body));
            } else if self.check(&Token::Else) {
                self.advance();
                self.eat(&Token::Colon)?;
                self.eat_newline();
                else_body = Some(self.parse_block()?);
                break;
            } else {
                break;
            }
        }

        Ok(Stmt::If {
            condition,
            then_body,
            elif_clauses,
            else_body,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::While)?;
        let condition = self.parse_expr()?;
        self.eat(&Token::Colon)?;
        self.eat_newline();
        let body = self.parse_block()?;
        Ok(Stmt::While { condition, body })
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::For)?;
        let var = self.expect_ident()?;
        self.eat(&Token::In)?;
        let iter = self.parse_expr()?;
        self.eat(&Token::Colon)?;
        self.eat_newline();
        let body = self.parse_block()?;
        Ok(Stmt::For { var, iter, body })
    }

    fn parse_return(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Return)?;
        if self.check(&Token::Newline) || self.check(&Token::Eof) {
            self.eat_newline();
            return Ok(Stmt::Return(None));
        }
        let first = self.parse_expr()?;
        // Support bare tuple return: return a, b  →  return (a, b)
        if self.check(&Token::Comma) {
            let mut elems = vec![first];
            while self.check(&Token::Comma) {
                self.advance();
                if self.check(&Token::Newline) || self.check(&Token::Eof) {
                    break;
                }
                elems.push(self.parse_expr()?);
            }
            self.eat_newline();
            return Ok(Stmt::Return(Some(Expr::Tuple(elems))));
        }
        self.eat_newline();
        Ok(Stmt::Return(Some(first)))
    }

    fn parse_import(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Import)?;
        if let Token::Str(path) = self.peek().clone() {
            // import "file.cool"  — file import
            self.advance();
            self.eat_newline();
            Ok(Stmt::Import(path))
        } else if let Token::Ident(name) = self.peek().clone() {
            // import math  or  import foo.bar  — built-in or dotted module
            self.advance();
            let mut parts = vec![name];
            while self.peek() == &Token::Dot {
                self.advance(); // consume '.'
                if let Token::Ident(part) = self.peek().clone() {
                    self.advance();
                    parts.push(part);
                } else {
                    return Err(format!("line {}: expected identifier after '.' in import", self.line()));
                }
            }
            self.eat_newline();
            Ok(Stmt::ImportModule(parts.join(".")))
        } else {
            Err(format!(
                "line {}: import expects a string path or module name",
                self.line()
            ))
        }
    }

    fn parse_try(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Try)?;
        self.eat(&Token::Colon)?;
        self.eat_newline();
        let body = self.parse_block()?;

        let mut handlers = Vec::new();
        let mut else_body = None;
        let mut finally_body = None;

        self.skip_newlines();
        // Parse except clauses
        while self.check(&Token::Except) {
            self.advance();
            let exc_type = if let Token::Ident(s) = self.peek().clone() {
                self.advance();
                Some(s)
            } else {
                None
            };
            let as_name = if self.check(&Token::As) {
                self.advance();
                Some(self.expect_ident()?)
            } else {
                None
            };
            self.eat(&Token::Colon)?;
            self.eat_newline();
            let handler_body = self.parse_block()?;
            handlers.push(ExceptHandler {
                exc_type,
                as_name,
                body: handler_body,
            });
            self.skip_newlines();
        }

        if self.check(&Token::Else) {
            self.advance();
            self.eat(&Token::Colon)?;
            self.eat_newline();
            else_body = Some(self.parse_block()?);
            self.skip_newlines();
        }

        if self.check(&Token::Finally) {
            self.advance();
            self.eat(&Token::Colon)?;
            self.eat_newline();
            finally_body = Some(self.parse_block()?);
        }

        if handlers.is_empty() && finally_body.is_none() {
            return Err(format!(
                "line {}: try requires at least one except or finally",
                self.line()
            ));
        }

        Ok(Stmt::Try {
            body,
            handlers,
            else_body,
            finally_body,
        })
    }

    fn parse_raise(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Raise)?;
        if self.check(&Token::Newline) || self.check(&Token::Eof) {
            self.eat_newline();
            return Ok(Stmt::Raise(None));
        }
        let expr = self.parse_expr()?;
        self.eat_newline();
        Ok(Stmt::Raise(Some(expr)))
    }

    fn parse_assert(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Assert)?;
        let condition = self.parse_expr()?;
        let message = if self.check(&Token::Comma) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.eat_newline();
        Ok(Stmt::Assert { condition, message })
    }

    fn parse_with(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::With)?;
        let expr = self.parse_expr()?;
        let as_name = if self.check(&Token::As) {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.eat(&Token::Colon)?;
        self.eat_newline();
        let body = self.parse_block()?;
        Ok(Stmt::With { expr, as_name, body })
    }

    fn parse_global(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Global)?;
        let mut names = vec![self.expect_ident()?];
        while self.check(&Token::Comma) {
            self.advance();
            names.push(self.expect_ident()?);
        }
        self.eat_newline();
        Ok(Stmt::Global(names))
    }

    fn parse_nonlocal(&mut self) -> Result<Stmt, String> {
        self.eat(&Token::Nonlocal)?;
        let mut names = vec![self.expect_ident()?];
        while self.check(&Token::Comma) {
            self.advance();
            names.push(self.expect_ident()?);
        }
        self.eat_newline();
        Ok(Stmt::Nonlocal(names))
    }

    /// Parse assignment or augmented assignment or a bare expression.
    fn parse_assign_or_expr(&mut self) -> Result<Stmt, String> {
        // Augmented assignment: ident <op>= expr
        if let Token::Ident(name) = self.peek().clone() {
            let aug_op = match self.token_at(1) {
                Token::PlusEq => Some(BinOp::Add),
                Token::MinusEq => Some(BinOp::Sub),
                Token::StarEq => Some(BinOp::Mul),
                Token::SlashEq => Some(BinOp::Div),
                _ => None,
            };
            if let Some(op) = aug_op {
                self.advance(); // consume ident
                self.advance(); // consume op=
                let value = self.parse_expr()?;
                self.eat_newline();
                return Ok(Stmt::AugAssign { name, op, value });
            }
        }

        // Tuple unpack: ident , ident , ... = expr
        if let Token::Ident(_) = self.peek().clone() {
            if let Token::Comma = self.token_at(1) {
                return self.parse_unpack_or_expr();
            }
        }

        // Parse LHS as an expression, then check for =
        let lhs = self.parse_expr()?;

        // Check for augmented assignment on subscript/attr targets: a[i] += x, obj.f += x
        let aug_op = match self.peek() {
            Token::PlusEq => Some(BinOp::Add),
            Token::MinusEq => Some(BinOp::Sub),
            Token::StarEq => Some(BinOp::Mul),
            Token::SlashEq => Some(BinOp::Div),
            _ => None,
        };
        if let Some(op) = aug_op {
            self.advance(); // consume op=
            let rhs = self.parse_expr()?;
            self.eat_newline();
            // Desugar: lhs op= rhs  →  lhs = lhs op rhs
            let combined = Expr::BinOp {
                op,
                left: Box::new(lhs.clone()),
                right: Box::new(rhs),
            };
            return match lhs {
                Expr::Ident(name) => Ok(Stmt::Assign { name, value: combined }),
                Expr::Index { object, index } => Ok(Stmt::SetItem {
                    object: *object,
                    index: *index,
                    value: combined,
                }),
                Expr::Attr { object, name } => Ok(Stmt::SetAttr {
                    object: *object,
                    name,
                    value: combined,
                }),
                _ => Err(format!("line {}: invalid augmented assignment target", self.line())),
            };
        }

        // Tuple targets: a[i], b.x = expr  (comma after a non-ident LHS)
        if self.check(&Token::Comma) {
            let mut targets = vec![lhs];
            while self.check(&Token::Comma) {
                self.advance();
                targets.push(self.parse_expr()?);
            }
            self.eat(&Token::Eq)?;
            // RHS may be a tuple: a, b = c, d
            let first_val = self.parse_expr()?;
            let value = if self.check(&Token::Comma) {
                let mut elems = vec![first_val];
                while self.check(&Token::Comma) {
                    self.advance();
                    if self.check(&Token::Newline) || self.check(&Token::Eof) {
                        break;
                    }
                    elems.push(self.parse_expr()?);
                }
                Expr::Tuple(elems)
            } else {
                first_val
            };
            self.eat_newline();
            // If all targets are plain idents, use the simpler Unpack node
            let all_idents: Option<Vec<String>> = targets
                .iter()
                .map(|t| if let Expr::Ident(n) = t { Some(n.clone()) } else { None })
                .collect();
            return if let Some(names) = all_idents {
                Ok(Stmt::Unpack { names, value })
            } else {
                Ok(Stmt::UnpackTargets { targets, value })
            };
        }

        if self.check(&Token::Eq) {
            self.advance(); // consume =
            let rhs = self.parse_expr()?;
            self.eat_newline();
            return match lhs {
                Expr::Ident(name) => Ok(Stmt::Assign { name, value: rhs }),
                Expr::Index { object, index } => Ok(Stmt::SetItem {
                    object: *object,
                    index: *index,
                    value: rhs,
                }),
                Expr::Attr { object, name } => Ok(Stmt::SetAttr {
                    object: *object,
                    name,
                    value: rhs,
                }),
                _ => Err(format!("line {}: invalid assignment target", self.line())),
            };
        }

        self.eat_newline();
        Ok(Stmt::Expr(lhs))
    }

    fn parse_unpack_or_expr(&mut self) -> Result<Stmt, String> {
        // Collect ident list up to =; if no = found, fall back to expr stmt
        let saved_pos = self.pos;
        let mut names = Vec::new();

        if let Token::Ident(n) = self.peek().clone() {
            self.advance();
            names.push(n);
        } else {
            self.pos = saved_pos;
            let e = self.parse_expr()?;
            self.eat_newline();
            return Ok(Stmt::Expr(e));
        }

        while self.check(&Token::Comma) {
            self.advance();
            if let Token::Ident(n) = self.peek().clone() {
                self.advance();
                names.push(n);
            } else {
                // Not an unpack — backtrack and parse as expr
                self.pos = saved_pos;
                let e = self.parse_expr()?;
                self.eat_newline();
                return Ok(Stmt::Expr(e));
            }
        }

        if self.check(&Token::Eq) {
            self.advance();
            let value = self.parse_expr()?;
            self.eat_newline();
            Ok(Stmt::Unpack { names, value })
        } else {
            // Not an assignment — backtrack
            self.pos = saved_pos;
            let e = self.parse_expr()?;
            self.eat_newline();
            Ok(Stmt::Expr(e))
        }
    }

    // ── Expressions (Pratt-style precedence climbing) ─────────────────────

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        // lambda: lambda [params]: expr
        if self.check(&Token::Lambda) {
            self.advance();
            let mut params = Vec::new();
            if !self.check(&Token::Colon) {
                params.push(self.parse_param()?);
                while self.check(&Token::Comma) {
                    self.advance();
                    if self.check(&Token::Colon) {
                        break;
                    }
                    params.push(self.parse_param()?);
                }
            }
            self.eat(&Token::Colon)?;
            let body = self.parse_expr()?;
            return Ok(Expr::Lambda {
                params,
                body: Box::new(body),
            });
        }
        let expr = self.parse_or()?;
        // ternary: expr if condition else expr
        if self.check(&Token::If) {
            self.advance();
            let condition = self.parse_or()?;
            self.eat(&Token::Else)?;
            let else_expr = self.parse_expr()?;
            return Ok(Expr::Ternary {
                condition: Box::new(condition),
                then_expr: Box::new(expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        while self.check(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_not()?;
        while self.check(&Token::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if self.check(&Token::Not) {
            self.advance();
            // "not in" as binary op
            if self.check(&Token::In) {
                // This case handled in comparison context; here just parse `not <expr>`
            }
            let expr = self.parse_not()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let left = self.parse_bitor()?;
        let op = match self.peek() {
            Token::EqEq => BinOp::Eq,
            Token::BangEq => BinOp::NotEq,
            Token::Lt => BinOp::Lt,
            Token::LtEq => BinOp::LtEq,
            Token::Gt => BinOp::Gt,
            Token::GtEq => BinOp::GtEq,
            Token::In => BinOp::In,
            Token::Not => {
                // Check for "not in"
                if matches!(self.token_at(1), Token::In) {
                    self.advance(); // consume 'not'
                    self.advance(); // consume 'in'
                    let right = self.parse_bitor()?;
                    return Ok(Expr::BinOp {
                        op: BinOp::NotIn,
                        left: Box::new(left),
                        right: Box::new(right),
                    });
                }
                return Ok(left);
            }
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_bitor()?;
        Ok(Expr::BinOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_bitor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitxor()?;
        while self.check(&Token::Pipe) {
            self.advance();
            let right = self.parse_bitxor()?;
            left = Expr::BinOp {
                op: BinOp::BitOr,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitand()?;
        while self.check(&Token::Caret) {
            self.advance();
            let right = self.parse_bitand()?;
            left = Expr::BinOp {
                op: BinOp::BitXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_shift()?;
        while self.check(&Token::Ampersand) {
            self.advance();
            let right = self.parse_shift()?;
            left = Expr::BinOp {
                op: BinOp::BitAnd,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Token::LtLt => BinOp::LShift,
                Token::GtGt => BinOp::RShift,
                _ => break,
            };
            self.advance();
            let right = self.parse_add()?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_mul()?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::SlashSlash => BinOp::FloorDiv,
                Token::Percent => BinOp::Mod,
                Token::StarStar => BinOp::Pow,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if self.check(&Token::Minus) {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }
        if self.check(&Token::Tilde) {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::BitNot,
                expr: Box::new(expr),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                Token::LParen => {
                    self.advance();
                    let (args, kwargs) = self.parse_arg_list()?;
                    self.eat(&Token::RParen)?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                        kwargs,
                    };
                }
                Token::LBracket => {
                    self.advance();
                    // Check for slice: obj[start:stop] or obj[:stop] or obj[start:]
                    if self.check(&Token::Colon) {
                        // obj[:stop] or obj[:]
                        self.advance();
                        let stop = if self.check(&Token::RBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        };
                        self.eat(&Token::RBracket)?;
                        expr = Expr::Slice {
                            object: Box::new(expr),
                            start: None,
                            stop,
                        };
                    } else {
                        let first = self.parse_expr()?;
                        if self.check(&Token::Colon) {
                            // obj[start:stop] or obj[start:]
                            self.advance();
                            let stop = if self.check(&Token::RBracket) {
                                None
                            } else {
                                Some(Box::new(self.parse_expr()?))
                            };
                            self.eat(&Token::RBracket)?;
                            expr = Expr::Slice {
                                object: Box::new(expr),
                                start: Some(Box::new(first)),
                                stop,
                            };
                        } else {
                            self.eat(&Token::RBracket)?;
                            expr = Expr::Index {
                                object: Box::new(expr),
                                index: Box::new(first),
                            };
                        }
                    }
                }
                Token::Dot => {
                    self.advance();
                    let name = self.expect_ident()?;
                    expr = Expr::Attr {
                        object: Box::new(expr),
                        name,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Returns (positional_args, keyword_args)
    fn parse_arg_list(&mut self) -> Result<(Vec<Expr>, Vec<(String, Expr)>), String> {
        let mut args = Vec::new();
        let mut kwargs = Vec::new();
        if self.check(&Token::RParen) {
            return Ok((args, kwargs));
        }

        loop {
            self.skip_collection_ws();
            if self.check(&Token::RParen) {
                break;
            }

            if self.check(&Token::StarStar) {
                // **dict — spread dict into kwargs; use sentinel name "**"
                self.advance();
                let expr = self.parse_expr()?;
                kwargs.push(("**".to_string(), expr));
            } else {
                let arg = self.parse_expr()?;
                if self.check(&Token::Eq) {
                    if let Expr::Ident(name) = arg {
                        self.advance();
                        let val = self.parse_expr()?;
                        kwargs.push((name, val));
                    } else {
                        return Err(format!(
                            "line {}: keyword argument name must be an identifier",
                            self.line()
                        ));
                    }
                } else {
                    if !kwargs.is_empty() {
                        return Err(format!(
                            "line {}: positional argument after keyword argument",
                            self.line()
                        ));
                    }
                    args.push(arg);
                }
            }

            if self.check(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok((args, kwargs))
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(Expr::Int(n))
            }
            Token::Float(f) => {
                self.advance();
                Ok(Expr::Float(f))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Token::FStr(s) => {
                self.advance();
                self.parse_fstring(s)
            }
            Token::Bool(b) => {
                self.advance();
                Ok(Expr::Bool(b))
            }
            Token::Nil => {
                self.advance();
                Ok(Expr::Nil)
            }
            Token::Ident(s) => {
                self.advance();
                Ok(Expr::Ident(s))
            }
            Token::LParen => {
                self.advance();
                self.skip_collection_ws();
                // Empty tuple ()
                if self.check(&Token::RParen) {
                    self.advance();
                    return Ok(Expr::Tuple(vec![]));
                }
                let first = self.parse_expr()?;
                self.skip_collection_ws();
                if self.check(&Token::Comma) {
                    // Tuple
                    let mut items = vec![first];
                    while self.check(&Token::Comma) {
                        self.advance();
                        self.skip_collection_ws();
                        if self.check(&Token::RParen) {
                            break;
                        }
                        items.push(self.parse_expr()?);
                    }
                    self.skip_collection_ws();
                    self.eat(&Token::RParen)?;
                    Ok(Expr::Tuple(items))
                } else {
                    self.eat(&Token::RParen)?;
                    Ok(first)
                }
            }
            Token::LBracket => {
                self.advance();
                self.skip_collection_ws();
                // Empty list
                if self.check(&Token::RBracket) {
                    self.advance();
                    return Ok(Expr::List(vec![]));
                }
                let first = self.parse_expr()?;
                self.skip_collection_ws();
                // List comprehension: [expr for var in iter (if cond)?]
                if self.check(&Token::For) {
                    self.advance();
                    let var = self.expect_ident()?;
                    self.eat(&Token::In)?;
                    // Use parse_or (not parse_expr) so `if` isn't consumed as ternary
                    let iter = self.parse_or()?;
                    let condition = if self.check(&Token::If) {
                        self.advance();
                        let mut cond = self.parse_or()?;
                        // Multiple `if` guards: chain with `and`
                        while self.check(&Token::If) {
                            self.advance();
                            let right = self.parse_or()?;
                            cond = Expr::BinOp {
                                op: BinOp::And,
                                left: Box::new(cond),
                                right: Box::new(right),
                            };
                        }
                        Some(Box::new(cond))
                    } else {
                        None
                    };
                    self.skip_collection_ws();
                    self.eat(&Token::RBracket)?;
                    return Ok(Expr::ListComp {
                        expr: Box::new(first),
                        var,
                        iter: Box::new(iter),
                        condition,
                    });
                }
                // Regular list
                let mut items = vec![first];
                while self.check(&Token::Comma) {
                    self.advance();
                    self.skip_collection_ws();
                    if self.check(&Token::RBracket) {
                        break;
                    }
                    items.push(self.parse_expr()?);
                }
                self.skip_collection_ws();
                self.eat(&Token::RBracket)?;
                Ok(Expr::List(items))
            }
            Token::LBrace => {
                self.advance();
                self.skip_collection_ws();
                let mut pairs = Vec::new();
                if !self.check(&Token::RBrace) {
                    let k = self.parse_expr()?;
                    self.eat(&Token::Colon)?;
                    let v = self.parse_expr()?;
                    pairs.push((k, v));
                    while self.check(&Token::Comma) {
                        self.advance();
                        self.skip_collection_ws();
                        if self.check(&Token::RBrace) {
                            break;
                        }
                        let k = self.parse_expr()?;
                        self.eat(&Token::Colon)?;
                        let v = self.parse_expr()?;
                        pairs.push((k, v));
                    }
                    self.skip_collection_ws();
                }
                self.eat(&Token::RBrace)?;
                Ok(Expr::Dict(pairs))
            }
            other => Err(format!(
                "line {}: unexpected token {:?} in expression",
                self.line(),
                other
            )),
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        if let Token::Ident(s) = self.peek().clone() {
            self.advance();
            Ok(s)
        } else {
            Err(format!(
                "line {}: expected identifier, got {:?}",
                self.line(),
                self.peek()
            ))
        }
    }

    /// Parse an f-string template into a list of literal/expr parts.
    fn parse_fstring(&self, raw: String) -> Result<Expr, String> {
        let mut parts = Vec::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        let mut lit = String::new();
        while i < chars.len() {
            if chars[i] == '{' {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                if !lit.is_empty() {
                    parts.push(FStringPart::Literal(lit.clone()));
                    lit.clear();
                }
                i += 1; // skip '{'
                let mut expr_src = String::new();
                let mut depth = 1;
                while i < chars.len() {
                    match chars[i] {
                        '{' => {
                            depth += 1;
                            expr_src.push('{');
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                            expr_src.push('}');
                        }
                        c => expr_src.push(c),
                    }
                    i += 1;
                }
                // Parse the inner expression
                let mut lex = crate::lexer::Lexer::new(&expr_src);
                let tokens = lex.tokenize().map_err(|e| e)?;
                let mut p = Parser::new(tokens);
                let expr = p.parse_expr()?;
                parts.push(FStringPart::Expr(expr));
            } else if chars[i] == '}' && i + 1 < chars.len() && chars[i + 1] == '}' {
                lit.push('}');
                i += 2;
            } else {
                lit.push(chars[i]);
                i += 1;
            }
        }
        if !lit.is_empty() {
            parts.push(FStringPart::Literal(lit));
        }
        Ok(Expr::FString(parts))
    }
}
