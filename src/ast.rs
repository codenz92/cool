/// Abstract Syntax Tree for Cool.

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Nil,
    Ident(String),

    BinOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        kwargs: Vec<(String, Expr)>,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    Slice {
        object: Box<Expr>,
        start: Option<Box<Expr>>,
        stop: Option<Box<Expr>>,
    },
    Attr {
        object: Box<Expr>,
        name: String,
    },
    List(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Tuple(Vec<Expr>),
    /// f-string: alternating literal strings and expressions
    FString(Vec<FStringPart>),
    /// lambda params: expr
    Lambda {
        params: Vec<Param>,
        body: Box<Expr>,
    },
    /// x if cond else y
    Ternary {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    /// [expr for var in iter (if cond)?]
    ListComp {
        expr: Box<Expr>,
        var: String,
        iter: Box<Expr>,
        condition: Option<Box<Expr>>,
    },
}

#[derive(Debug, Clone)]
pub enum FStringPart {
    Literal(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    FloorDiv,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    In,
    NotIn,
    BitAnd,
    BitOr,
    BitXor,
    LShift,
    RShift,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

/// One except clause in a try statement.
#[derive(Debug, Clone)]
pub struct ExceptHandler {
    /// The exception type to catch (None = bare `except:`)
    pub exc_type: Option<String>,
    /// `as name` binding
    pub as_name: Option<String>,
    pub body: Vec<Stmt>,
}

/// A function parameter — name plus optional default expression.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
    pub is_vararg: bool, // *args
    pub is_kwarg: bool,  // **kwargs
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// Pseudo-statement: records the source line for error messages.
    SetLine(usize),

    Expr(Expr),
    Assign {
        name: String,
        value: Expr,
    },
    /// obj[index] = value
    SetItem {
        object: Expr,
        index: Expr,
        value: Expr,
    },
    /// obj.name = value
    SetAttr {
        object: Expr,
        name: String,
        value: Expr,
    },
    /// Augmented assignment: name += expr  etc.
    AugAssign {
        name: String,
        op: BinOp,
        value: Expr,
    },
    /// Tuple unpack: a, b, c = expr
    Unpack {
        names: Vec<String>,
        value: Expr,
    },
    /// Tuple unpack with non-trivial targets: a[i], obj.x = expr
    UnpackTargets {
        targets: Vec<Expr>,
        value: Expr,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        elif_clauses: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        var: String,
        iter: Expr,
        body: Vec<Stmt>,
    },
    FnDef {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
    },
    /// class Name(Parent): ...
    Class {
        name: String,
        parent: Option<String>,
        body: Vec<Stmt>,
    },
    /// struct Name:\n    field: type\n    ...
    Struct {
        name: String,
        fields: Vec<(String, String)>, // (field_name, type_name)
    },
    /// try / except / else / finally
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        else_body: Option<Vec<Stmt>>,
        finally_body: Option<Vec<Stmt>>,
    },
    /// raise expr  or bare  raise
    Raise(Option<Expr>),
    /// import "file.cool"
    Import(String),
    /// import ModuleName  (built-in modules)
    ImportModule(String),
    Pass,
    /// assert condition [, message]
    Assert {
        condition: Expr,
        message: Option<Expr>,
    },
    /// with expr as name: body
    With {
        expr: Expr,
        as_name: Option<String>,
        body: Vec<Stmt>,
    },
    /// global x, y
    Global(Vec<String>),
    /// nonlocal x, y
    Nonlocal(Vec<String>),
}

pub type Program = Vec<Stmt>;
