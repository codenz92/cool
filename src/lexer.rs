/// Lexer for Cool — tokenizes source text into a flat list of tokens.
/// Cool uses Python-style indentation for blocks.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    FStr(String), // f"..." raw template
    Bool(bool),
    Nil,

    // Identifiers & keywords
    Ident(String),
    Def,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    And,
    Or,
    Not,
    Pass,
    Break,
    Continue,
    Import,
    Class,
    Struct,
    Try,
    Except,
    Finally,
    Raise,
    As,
    Assert,
    With,
    Global,
    Nonlocal,
    Lambda,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    SlashSlash, // //  floor division
    Ampersand,  // &
    Pipe,       // |
    Caret,      // ^
    Tilde,      // ~
    LtLt,       // <<
    GtGt,       // >>

    // Punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    StarStar, // **

    // Layout
    Newline,
    Indent,
    Dedent,

    // End
    Eof,
}

#[derive(Debug, Clone)]
pub struct Span {
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Tok {
    pub token: Token,
    pub span: Span,
}

pub struct Lexer {
    src: Vec<char>,
    pos: usize,
    line: usize,
    indent_stack: Vec<usize>,
    pending: Vec<Tok>,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            src: source.chars().collect(),
            pos: 0,
            line: 1,
            indent_stack: vec![0],
            pending: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.src.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.src.get(self.pos).copied();
        self.pos += 1;
        c
    }

    fn tok(&self, token: Token) -> Tok {
        Tok {
            token,
            span: Span { line: self.line },
        }
    }

    fn skip_comment(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn read_string(&mut self, quote: char) -> Result<Tok, String> {
        let line = self.line;
        // Triple-quoted strings
        if self.peek() == Some(quote) && self.peek2() == Some(quote) {
            self.advance();
            self.advance();
            let mut s = String::new();
            loop {
                match self.advance() {
                    None => return Err(format!("line {}: unterminated triple-quoted string", line)),
                    Some(c) if c == quote => {
                        if self.peek() == Some(quote) && self.peek2() == Some(quote) {
                            self.advance();
                            self.advance();
                            break;
                        }
                        s.push(c);
                    }
                    Some('\n') => {
                        self.line += 1;
                        s.push('\n');
                    }
                    Some(c) => s.push(c),
                }
            }
            return Ok(Tok {
                token: Token::Str(s),
                span: Span { line },
            });
        }
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err(format!("line {}: unterminated string", line)),
                Some(c) if c == quote => break,
                Some('\\') => {
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('0') => s.push('\0'),
                        Some('x') => {
                            // \xHH hex escape
                            let h1 = self.advance().unwrap_or('0');
                            let h2 = self.advance().unwrap_or('0');
                            let hex = format!("{}{}", h1, h2);
                            match u8::from_str_radix(&hex, 16) {
                                Ok(byte) => s.push(byte as char),
                                Err(_) => {
                                    s.push('\\');
                                    s.push('x');
                                    s.push(h1);
                                    s.push(h2);
                                }
                            }
                        }
                        Some(q) if q == quote => s.push(q),
                        Some(c) => {
                            s.push('\\');
                            s.push(c);
                        }
                        None => return Err(format!("line {}: unterminated escape", line)),
                    }
                }
                Some(c) => s.push(c),
            }
        }
        Ok(Tok {
            token: Token::Str(s),
            span: Span { line },
        })
    }

    fn read_number(&mut self, first: char) -> Tok {
        // Hex: 0x..., binary: 0b..., octal: 0o...
        if first == '0' {
            match self.peek() {
                Some('x') | Some('X') => {
                    self.advance(); // consume 'x'
                    let mut s = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_hexdigit() {
                            s.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let n = i64::from_str_radix(&s, 16).unwrap_or(0);
                    return self.tok(Token::Int(n));
                }
                Some('b') | Some('B') => {
                    self.advance();
                    let mut s = String::new();
                    while let Some(c) = self.peek() {
                        if c == '0' || c == '1' {
                            s.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let n = i64::from_str_radix(&s, 2).unwrap_or(0);
                    return self.tok(Token::Int(n));
                }
                Some('o') | Some('O') => {
                    self.advance();
                    let mut s = String::new();
                    while let Some(c) = self.peek() {
                        if c >= '0' && c <= '7' {
                            s.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let n = i64::from_str_radix(&s, 8).unwrap_or(0);
                    return self.tok(Token::Int(n));
                }
                _ => {}
            }
        }
        let mut s = String::from(first);
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else if c == '.' && !is_float && self.peek2().map_or(false, |d| d.is_ascii_digit()) {
                is_float = true;
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if is_float {
            self.tok(Token::Float(s.parse().unwrap()))
        } else {
            self.tok(Token::Int(s.parse().unwrap()))
        }
    }

    fn read_ident(&mut self, first: char) -> Tok {
        let mut s = String::from(first);
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        let token = match s.as_str() {
            "def" => Token::Def,
            "return" => Token::Return,
            "if" => Token::If,
            "elif" => Token::Elif,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "pass" => Token::Pass,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "import" => Token::Import,
            "class" => Token::Class,
            "struct" => Token::Struct,
            "try" => Token::Try,
            "except" => Token::Except,
            "finally" => Token::Finally,
            "raise" => Token::Raise,
            "as" => Token::As,
            "assert" => Token::Assert,
            "with" => Token::With,
            "global" => Token::Global,
            "nonlocal" => Token::Nonlocal,
            "lambda" => Token::Lambda,
            "true" => Token::Bool(true),
            "false" => Token::Bool(false),
            "nil" => Token::Nil,
            _ => Token::Ident(s),
        };
        self.tok(token)
    }

    fn handle_indent(&mut self) -> Vec<Tok> {
        let mut col = 0usize;
        loop {
            match self.peek() {
                Some(' ') => {
                    col += 1;
                    self.advance();
                }
                Some('\t') => {
                    col += 4;
                    self.advance();
                }
                _ => break,
            }
        }
        match self.peek() {
            None | Some('\n') => return vec![],
            Some('#') => {
                self.skip_comment();
                // Consume the trailing newline and treat the whole comment line as blank.
                if self.peek() == Some('\n') {
                    self.advance();
                    self.line += 1;
                }
                return self.handle_indent();
            }
            _ => {}
        }

        let line = self.line;
        let mut toks = vec![];
        let current = *self.indent_stack.last().unwrap();
        if col > current {
            self.indent_stack.push(col);
            toks.push(Tok {
                token: Token::Indent,
                span: Span { line },
            });
        } else {
            while col < *self.indent_stack.last().unwrap() {
                self.indent_stack.pop();
                toks.push(Tok {
                    token: Token::Dedent,
                    span: Span { line },
                });
            }
        }
        toks
    }

    pub fn tokenize(&mut self) -> Result<Vec<Tok>, String> {
        let mut tokens: Vec<Tok> = Vec::new();
        let mut at_line_start = true;

        loop {
            if !self.pending.is_empty() {
                tokens.extend(self.pending.drain(..));
            }

            if at_line_start {
                let indent_toks = self.handle_indent();
                tokens.extend(indent_toks);
                at_line_start = false;
            }

            match self.peek() {
                None => {
                    let line = self.line;
                    while self.indent_stack.len() > 1 {
                        self.indent_stack.pop();
                        tokens.push(Tok {
                            token: Token::Dedent,
                            span: Span { line },
                        });
                    }
                    tokens.push(self.tok(Token::Eof));
                    break;
                }
                Some('\n') => {
                    self.advance();
                    self.line += 1;
                    tokens.push(self.tok(Token::Newline));
                    at_line_start = true;
                }
                Some('\r') => {
                    self.advance();
                }
                Some(' ') | Some('\t') => {
                    self.advance();
                }
                Some('\\') if self.peek2() == Some('\n') => {
                    // Line continuation — consume both, skip the newline silently
                    self.advance(); // consume '\'
                    self.advance(); // consume '\n'
                    self.line += 1;
                    // Skip any leading whitespace on the next line too
                }
                Some('#') => self.skip_comment(),
                Some('"') | Some('\'') => {
                    let q = self.advance().unwrap();
                    tokens.push(self.read_string(q)?);
                }
                Some(c) if c.is_ascii_digit() => {
                    self.advance();
                    tokens.push(self.read_number(c));
                }
                Some(c) if c.is_alphabetic() || c == '_' => {
                    self.advance();
                    // f-string: f"..." or f'...'
                    if (c == 'f' || c == 'F') && matches!(self.peek(), Some('"') | Some('\'')) {
                        let q = self.advance().unwrap();
                        let tok = self.read_string(q)?;
                        // Re-wrap as FStr with the same raw content
                        if let Token::Str(s) = tok.token {
                            tokens.push(Tok {
                                token: Token::FStr(s),
                                span: tok.span,
                            });
                        }
                    } else {
                        tokens.push(self.read_ident(c));
                    }
                }
                Some('+') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::PlusEq));
                    } else {
                        tokens.push(self.tok(Token::Plus));
                    }
                }
                Some('-') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::MinusEq));
                    } else {
                        tokens.push(self.tok(Token::Minus));
                    }
                }
                Some('*') => {
                    self.advance();
                    if self.peek() == Some('*') {
                        self.advance();
                        tokens.push(self.tok(Token::StarStar));
                    } else if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::StarEq));
                    } else {
                        tokens.push(self.tok(Token::Star));
                    }
                }
                Some('/') => {
                    self.advance();
                    if self.peek() == Some('/') {
                        self.advance();
                        tokens.push(self.tok(Token::SlashSlash));
                    } else if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::SlashEq));
                    } else {
                        tokens.push(self.tok(Token::Slash));
                    }
                }
                Some('%') => {
                    self.advance();
                    tokens.push(self.tok(Token::Percent));
                }
                Some('&') => {
                    self.advance();
                    tokens.push(self.tok(Token::Ampersand));
                }
                Some('|') => {
                    self.advance();
                    tokens.push(self.tok(Token::Pipe));
                }
                Some('^') => {
                    self.advance();
                    tokens.push(self.tok(Token::Caret));
                }
                Some('~') => {
                    self.advance();
                    tokens.push(self.tok(Token::Tilde));
                }
                Some('(') => {
                    self.advance();
                    tokens.push(self.tok(Token::LParen));
                }
                Some(')') => {
                    self.advance();
                    tokens.push(self.tok(Token::RParen));
                }
                Some('[') => {
                    self.advance();
                    tokens.push(self.tok(Token::LBracket));
                }
                Some(']') => {
                    self.advance();
                    tokens.push(self.tok(Token::RBracket));
                }
                Some('{') => {
                    self.advance();
                    tokens.push(self.tok(Token::LBrace));
                }
                Some('}') => {
                    self.advance();
                    tokens.push(self.tok(Token::RBrace));
                }
                Some(',') => {
                    self.advance();
                    tokens.push(self.tok(Token::Comma));
                }
                Some(':') => {
                    self.advance();
                    tokens.push(self.tok(Token::Colon));
                }
                Some('.') => {
                    self.advance();
                    tokens.push(self.tok(Token::Dot));
                }
                Some('=') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::EqEq));
                    } else {
                        tokens.push(self.tok(Token::Eq));
                    }
                }
                Some('!') => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::BangEq));
                    } else {
                        return Err(format!("line {}: unexpected '!'", self.line));
                    }
                }
                Some('<') => {
                    self.advance();
                    if self.peek() == Some('<') {
                        self.advance();
                        tokens.push(self.tok(Token::LtLt));
                    } else if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::LtEq));
                    } else {
                        tokens.push(self.tok(Token::Lt));
                    }
                }
                Some('>') => {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        tokens.push(self.tok(Token::GtGt));
                    } else if self.peek() == Some('=') {
                        self.advance();
                        tokens.push(self.tok(Token::GtEq));
                    } else {
                        tokens.push(self.tok(Token::Gt));
                    }
                }
                Some(c) => {
                    return Err(format!("line {}: unexpected character '{}'", self.line, c));
                }
            }
        }

        Ok(tokens)
    }
}
