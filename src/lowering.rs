use crate::ast::{BinOp, EnumVariant, Expr, MatchArm, Param, Pattern, Program, Stmt};
use std::collections::HashMap;

#[derive(Clone, Default)]
struct EnumRegistry {
    enums: HashMap<String, EnumShape>,
    variants: HashMap<String, Option<String>>,
}

#[derive(Clone)]
struct EnumShape {
    name: String,
    variants: HashMap<String, Vec<String>>,
}

pub fn lower_program(program: &Program) -> Result<Program, String> {
    let mut counter = 0usize;
    lower_block(program, &EnumRegistry::default(), &mut counter)
}

fn lower_block(stmts: &[Stmt], inherited: &EnumRegistry, counter: &mut usize) -> Result<Vec<Stmt>, String> {
    let mut registry = inherited.clone();
    collect_block_enums(stmts, &mut registry);

    let mut out = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::SetLine(_) => out.push(stmt.clone()),
            Stmt::Visibility { visibility, stmt } => match stmt.as_ref() {
                Stmt::Enum { .. } => out.extend(lower_stmt(stmt, &registry, counter)?),
                Stmt::Trait { .. } => {}
                inner => {
                    for lowered in lower_stmt(inner, &registry, counter)? {
                        out.push(Stmt::Visibility {
                            visibility: *visibility,
                            stmt: Box::new(lowered),
                        });
                    }
                }
            },
            other => out.extend(lower_stmt(other, &registry, counter)?),
        }
    }
    Ok(out)
}

fn collect_block_enums(stmts: &[Stmt], registry: &mut EnumRegistry) {
    for stmt in stmts {
        match stmt {
            Stmt::Enum { name, variants, .. } => registry.register(EnumShape::new(name, variants)),
            Stmt::Visibility { stmt, .. } => {
                if let Stmt::Enum { name, variants, .. } = stmt.as_ref() {
                    registry.register(EnumShape::new(name, variants));
                }
            }
            _ => {}
        }
    }
}

fn lower_stmt(stmt: &Stmt, registry: &EnumRegistry, counter: &mut usize) -> Result<Vec<Stmt>, String> {
    match stmt {
        Stmt::FnDef {
            name,
            type_params,
            params,
            return_type,
            section,
            entry,
            body,
        } => Ok(vec![Stmt::FnDef {
            name: name.clone(),
            type_params: type_params.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            section: section.clone(),
            entry: entry.clone(),
            body: lower_block(body, registry, counter)?,
        }]),
        Stmt::Class {
            name,
            parent,
            implements,
            body,
        } => Ok(vec![Stmt::Class {
            name: name.clone(),
            parent: parent.clone(),
            implements: implements.clone(),
            body: lower_block(body, registry, counter)?,
        }]),
        Stmt::Struct {
            name,
            type_params,
            fields,
            is_packed,
        } if !type_params.is_empty() => Ok(vec![lower_struct_like_class(name, fields, false, *is_packed)]),
        Stmt::Union {
            name,
            type_params,
            fields,
        } if !type_params.is_empty() => Ok(vec![lower_struct_like_class(name, fields, true, false)]),
        Stmt::If {
            condition,
            then_body,
            elif_clauses,
            else_body,
        } => Ok(vec![Stmt::If {
            condition: condition.clone(),
            then_body: lower_block(then_body, registry, counter)?,
            elif_clauses: elif_clauses
                .iter()
                .map(|(expr, body)| Ok((expr.clone(), lower_block(body, registry, counter)?)))
                .collect::<Result<_, String>>()?,
            else_body: else_body
                .as_ref()
                .map(|body| lower_block(body, registry, counter))
                .transpose()?,
        }]),
        Stmt::While { condition, body } => Ok(vec![Stmt::While {
            condition: condition.clone(),
            body: lower_block(body, registry, counter)?,
        }]),
        Stmt::For { var, iter, body } => Ok(vec![Stmt::For {
            var: var.clone(),
            iter: iter.clone(),
            body: lower_block(body, registry, counter)?,
        }]),
        Stmt::With { expr, as_name, body } => Ok(vec![Stmt::With {
            expr: expr.clone(),
            as_name: as_name.clone(),
            body: lower_block(body, registry, counter)?,
        }]),
        Stmt::Try {
            body,
            handlers,
            else_body,
            finally_body,
        } => Ok(vec![Stmt::Try {
            body: lower_block(body, registry, counter)?,
            handlers: handlers
                .iter()
                .map(|handler| {
                    Ok(crate::ast::ExceptHandler {
                        exc_type: handler.exc_type.clone(),
                        as_name: handler.as_name.clone(),
                        body: lower_block(&handler.body, registry, counter)?,
                    })
                })
                .collect::<Result<_, String>>()?,
            else_body: else_body
                .as_ref()
                .map(|body| lower_block(body, registry, counter))
                .transpose()?,
            finally_body: finally_body
                .as_ref()
                .map(|body| lower_block(body, registry, counter))
                .transpose()?,
        }]),
        Stmt::Enum { name, variants, .. } => Ok(lower_enum(name, variants)),
        Stmt::Trait { .. } => Ok(Vec::new()),
        Stmt::Match { value, arms } => lower_match(value, arms, registry, counter),
        _ => Ok(vec![stmt.clone()]),
    }
}

fn lower_enum(name: &str, variants: &[EnumVariant]) -> Vec<Stmt> {
    let mut out = Vec::new();
    for variant in variants {
        let helper_name = enum_variant_helper_name(name, &variant.name);
        let mut params = vec![Param {
            name: "self".to_string(),
            default: None,
            is_vararg: false,
            is_kwarg: false,
            type_name: None,
        }];
        let mut init_body = vec![
            Stmt::SetAttr {
                object: Expr::Ident("self".to_string()),
                name: "enum".to_string(),
                value: Expr::Str(name.to_string()),
            },
            Stmt::SetAttr {
                object: Expr::Ident("self".to_string()),
                name: "tag".to_string(),
                value: Expr::Str(variant.name.clone()),
            },
        ];
        for (field_name, _) in &variant.fields {
            params.push(Param {
                name: field_name.clone(),
                default: None,
                is_vararg: false,
                is_kwarg: false,
                type_name: None,
            });
            init_body.push(Stmt::SetAttr {
                object: Expr::Ident("self".to_string()),
                name: field_name.clone(),
                value: Expr::Ident(field_name.clone()),
            });
        }
        out.push(Stmt::Class {
            name: helper_name.clone(),
            parent: None,
            implements: Vec::new(),
            body: vec![Stmt::FnDef {
                name: "__init__".to_string(),
                type_params: Vec::new(),
                params,
                return_type: None,
                section: None,
                entry: None,
                body: init_body,
            }],
        });
    }

    let mut namespace_body = Vec::new();
    for variant in variants {
        let helper_name = enum_variant_helper_name(name, &variant.name);
        let value = if variant.fields.is_empty() {
            Expr::Call {
                callee: Box::new(Expr::Ident(helper_name)),
                args: Vec::new(),
                kwargs: Vec::new(),
            }
        } else {
            Expr::Ident(helper_name)
        };
        namespace_body.push(Stmt::Assign {
            name: variant.name.clone(),
            value,
        });
    }
    namespace_body.push(Stmt::Assign {
        name: "__variants__".to_string(),
        value: Expr::List(variants.iter().map(|variant| Expr::Str(variant.name.clone())).collect()),
    });
    out.push(Stmt::Class {
        name: name.to_string(),
        parent: None,
        implements: Vec::new(),
        body: namespace_body,
    });
    out
}

fn lower_struct_like_class(name: &str, fields: &[(String, String)], zero_defaulted: bool, _is_packed: bool) -> Stmt {
    let mut params = vec![Param {
        name: "self".to_string(),
        default: None,
        is_vararg: false,
        is_kwarg: false,
        type_name: None,
    }];
    let mut init_body = Vec::new();
    for (field_name, type_name) in fields {
        params.push(Param {
            name: field_name.clone(),
            default: if zero_defaulted {
                Some(match type_name.as_str() {
                    "f32" | "f64" | "float" => Expr::Float(0.0),
                    "bool" => Expr::Bool(false),
                    _ => Expr::Int(0),
                })
            } else {
                None
            },
            is_vararg: false,
            is_kwarg: false,
            type_name: None,
        });
        let value = match type_name.as_str() {
            "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" | "isize" | "usize" => Expr::Call {
                callee: Box::new(Expr::Ident("int".to_string())),
                args: vec![Expr::Ident(field_name.clone())],
                kwargs: Vec::new(),
            },
            "f32" | "f64" | "float" => Expr::Call {
                callee: Box::new(Expr::Ident("float".to_string())),
                args: vec![Expr::Ident(field_name.clone())],
                kwargs: Vec::new(),
            },
            "bool" => Expr::Call {
                callee: Box::new(Expr::Ident("bool".to_string())),
                args: vec![Expr::Ident(field_name.clone())],
                kwargs: Vec::new(),
            },
            _ => Expr::Ident(field_name.clone()),
        };
        init_body.push(Stmt::SetAttr {
            object: Expr::Ident("self".to_string()),
            name: field_name.clone(),
            value,
        });
    }
    Stmt::Class {
        name: name.to_string(),
        parent: None,
        implements: Vec::new(),
        body: vec![Stmt::FnDef {
            name: "__init__".to_string(),
            type_params: Vec::new(),
            params,
            return_type: None,
            section: None,
            entry: None,
            body: init_body,
        }],
    }
}

fn lower_match(
    value: &Expr,
    arms: &[MatchArm],
    registry: &EnumRegistry,
    counter: &mut usize,
) -> Result<Vec<Stmt>, String> {
    let temp_name = format!("__match_value_{counter}");
    *counter += 1;
    let temp_expr = Expr::Ident(temp_name.clone());
    let mut out = vec![Stmt::VarDecl {
        name: temp_name,
        type_name: None,
        value: value.clone(),
        is_const: true,
    }];

    let mut nested: Option<Vec<Stmt>> = None;
    for arm in arms.iter().rev() {
        let lowered = lower_pattern(&arm.pattern, &temp_expr, registry)?;
        let mut then_body = lowered.bindings;
        then_body.extend(lower_block(&arm.body, registry, counter)?);
        nested = Some(if matches!(lowered.condition, Expr::Bool(true)) {
            then_body
        } else {
            vec![Stmt::If {
                condition: lowered.condition,
                then_body,
                elif_clauses: Vec::new(),
                else_body: nested.take(),
            }]
        });
    }
    if let Some(stmts) = nested {
        out.extend(stmts);
    }
    Ok(out)
}

struct LoweredPattern {
    condition: Expr,
    bindings: Vec<Stmt>,
}

fn lower_pattern(pattern: &Pattern, value: &Expr, registry: &EnumRegistry) -> Result<LoweredPattern, String> {
    match pattern {
        Pattern::Wildcard => Ok(LoweredPattern {
            condition: Expr::Bool(true),
            bindings: Vec::new(),
        }),
        Pattern::Capture(name) => Ok(LoweredPattern {
            condition: Expr::Bool(true),
            bindings: vec![Stmt::Assign {
                name: name.clone(),
                value: value.clone(),
            }],
        }),
        Pattern::Literal(literal) => Ok(LoweredPattern {
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(value.clone()),
                right: Box::new(literal.clone()),
            },
            bindings: Vec::new(),
        }),
        Pattern::Variant {
            enum_name,
            variant,
            fields,
        } => {
            let resolved_enum = enum_name
                .clone()
                .or_else(|| registry.resolve_variant(variant))
                .ok_or_else(|| format!("match: cannot resolve enum for variant pattern '{variant}'"))?;
            let helper_name = enum_variant_helper_name(&resolved_enum, variant);
            let mut condition = call_builtin("isinstance", vec![value.clone(), Expr::Ident(helper_name)]);

            let mut bindings = Vec::new();
            if !fields.is_empty() {
                let field_names = registry
                    .field_names(&resolved_enum, variant)
                    .ok_or_else(|| format!("match: unknown enum variant '{}.{}'", resolved_enum, variant))?;
                if field_names.len() != fields.len() {
                    return Err(format!(
                        "match: variant '{}.{}' expects {} field(s), got {}",
                        resolved_enum,
                        variant,
                        field_names.len(),
                        fields.len()
                    ));
                }
                for (field_pattern, field_name) in fields.iter().zip(field_names.iter()) {
                    let field_expr = Expr::Attr {
                        object: Box::new(value.clone()),
                        name: field_name.clone(),
                    };
                    let lowered = lower_pattern(field_pattern, &field_expr, registry)?;
                    condition = and_expr(condition, lowered.condition);
                    bindings.extend(lowered.bindings);
                }
            }
            Ok(LoweredPattern { condition, bindings })
        }
    }
}

fn call_builtin(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident(name.to_string())),
        args,
        kwargs: Vec::new(),
    }
}

fn and_expr(left: Expr, right: Expr) -> Expr {
    match (&left, &right) {
        (Expr::Bool(true), _) => right,
        (_, Expr::Bool(true)) => left,
        _ => Expr::BinOp {
            op: BinOp::And,
            left: Box::new(left),
            right: Box::new(right),
        },
    }
}

fn enum_variant_helper_name(enum_name: &str, variant_name: &str) -> String {
    format!("__{}_{}", enum_name, variant_name)
}

impl EnumShape {
    fn new(name: &str, variants: &[EnumVariant]) -> Self {
        Self {
            name: name.to_string(),
            variants: variants
                .iter()
                .map(|variant| {
                    (
                        variant.name.clone(),
                        variant
                            .fields
                            .iter()
                            .map(|(field_name, _)| field_name.clone())
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

impl EnumRegistry {
    fn register(&mut self, shape: EnumShape) {
        for variant_name in shape.variants.keys() {
            self.variants
                .entry(variant_name.clone())
                .and_modify(|entry| *entry = None)
                .or_insert_with(|| Some(shape.name.clone()));
        }
        self.enums.insert(shape.name.clone(), shape);
    }

    fn resolve_variant(&self, variant_name: &str) -> Option<String> {
        self.variants.get(variant_name).cloned().flatten()
    }

    fn field_names(&self, enum_name: &str, variant_name: &str) -> Option<&Vec<String>> {
        self.enums.get(enum_name)?.variants.get(variant_name)
    }
}
