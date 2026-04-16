use crate::ast::{Expr, Program, Stmt};
use inkwell::context::Context;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;
use std::path::Path;

pub fn compile_program(program: &Program, output_path: &Path) -> Result<(), String> {
    // ── LLVM setup ────────────────────────────────────────────────────────────
    let context = Context::create();
    let module = context.create_module("cool_program");
    let builder = context.create_builder();

    let i32_type = context.i32_type();
    let ptr_type = context.i8_type().ptr_type(inkwell::AddressSpace::default());

    // Declare `puts(const char*) -> i32`
    let puts_type = i32_type.fn_type(&[ptr_type.into()], false);
    let puts_fn = module.add_function("puts", puts_type, None);

    // Declare `printf(const char*, ...) -> i32` for printing numbers
    let printf_type = i32_type.fn_type(&[ptr_type.into()], true);
    let printf_fn = module.add_function("printf", printf_type, None);

    // Build `main()`
    let main_type = i32_type.fn_type(&[], false);
    let main_fn = module.add_function("main", main_type, None);
    let entry = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry);

    // ── Walk the AST ──────────────────────────────────────────────────────────
    let mut str_counter = 0usize;

    for stmt in program {
        compile_stmt(
            stmt,
            &context,
            &module,
            &builder,
            puts_fn,
            printf_fn,
            &mut str_counter,
        )?;
    }

    // Return 0
    builder.build_return(Some(&i32_type.const_int(0, false))).unwrap();

    // ── Emit native binary ────────────────────────────────────────────────────
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| format!("LLVM init error: {e}"))?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| format!("Target error: {e}"))?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or("Failed to create target machine")?;

    let obj_path = output_path.with_extension("o");
    machine
        .write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| format!("Write error: {e}"))?;

    let status = std::process::Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(output_path)
        .status()
        .map_err(|e| format!("Linker error: {e}"))?;

    if !status.success() {
        return Err("Linking failed".to_string());
    }

    std::fs::remove_file(&obj_path).ok();
    Ok(())
}

// ── Statement compiler ────────────────────────────────────────────────────────

fn compile_stmt<'ctx>(
    stmt: &Stmt,
    context: &'ctx Context,
    module: &inkwell::module::Module<'ctx>,
    builder: &inkwell::builder::Builder<'ctx>,
    puts_fn: inkwell::values::FunctionValue<'ctx>,
    printf_fn: inkwell::values::FunctionValue<'ctx>,
    str_counter: &mut usize,
) -> Result<(), String> {
    match stmt {
        // Skip line-number markers
        Stmt::SetLine(_) => {}

        // Expression statement — only care about Call for now
        Stmt::Expr(expr) => {
            compile_expr_stmt(expr, context, module, builder, puts_fn, printf_fn, str_counter)?;
        }

        // Everything else is unsupported for now
        other => {
            return Err(format!(
                "Unsupported statement for LLVM compilation: {:?}",
                other
            ));
        }
    }
    Ok(())
}

// ── Expression statement compiler ────────────────────────────────────────────

fn compile_expr_stmt<'ctx>(
    expr: &Expr,
    context: &'ctx Context,
    module: &inkwell::module::Module<'ctx>,
    builder: &inkwell::builder::Builder<'ctx>,
    puts_fn: inkwell::values::FunctionValue<'ctx>,
    printf_fn: inkwell::values::FunctionValue<'ctx>,
    str_counter: &mut usize,
) -> Result<(), String> {
    match expr {
        // print(arg1, arg2, ...)
        Expr::Call { callee, args, .. } => {
            if let Expr::Ident(name) = callee.as_ref() {
                if name == "print" {
                    compile_print(args, context, module, builder, puts_fn, printf_fn, str_counter)?;
                    return Ok(());
                }
            }
            Err(format!("Unsupported call for LLVM compilation: {:?}", callee))
        }
        other => Err(format!(
            "Unsupported expression for LLVM compilation: {:?}",
            other
        )),
    }
}

// ── print() handler ───────────────────────────────────────────────────────────
//
// Supports:
//   print("hello")          → puts("hello")
//   print(42)               → printf("%lld\n", 42)
//   print(3.14)             → printf("%g\n", 3.14)
//   print("x =", 42)       → printf("x = %lld\n", 42)   (simple 2-arg case)
//   print()                 → puts("")

fn compile_print<'ctx>(
    args: &[Expr],
    context: &'ctx Context,
    module: &inkwell::module::Module<'ctx>,
    builder: &inkwell::builder::Builder<'ctx>,
    puts_fn: inkwell::values::FunctionValue<'ctx>,
    printf_fn: inkwell::values::FunctionValue<'ctx>,
    str_counter: &mut usize,
) -> Result<(), String> {
    let i32_type = context.i32_type();
    let i64_type = context.i64_type();
    let f64_type = context.f64_type();

    match args {
        // print()  →  puts("")
        [] => {
            let empty = builder.build_global_string_ptr("", "empty").unwrap();
            builder.build_call(puts_fn, &[empty.as_pointer_value().into()], "").unwrap();
        }

        // print("hello")  →  puts("hello")
        [Expr::Str(s)] => {
            let name = format!("str_{}", str_counter);
            *str_counter += 1;
            let global = builder.build_global_string_ptr(s, &name).unwrap();
            builder.build_call(puts_fn, &[global.as_pointer_value().into()], "").unwrap();
        }

        // print(42)  →  printf("%lld\n", 42)
        [Expr::Int(n)] => {
            let fmt = builder.build_global_string_ptr("%lld\n", "fmt_int").unwrap();
            let val = i64_type.const_int(*n as u64, true);
            builder
                .build_call(printf_fn, &[fmt.as_pointer_value().into(), val.into()], "")
                .unwrap();
        }

        // print(3.14)  →  printf("%g\n", 3.14)
        [Expr::Float(f)] => {
            let fmt = builder.build_global_string_ptr("%g\n", "fmt_float").unwrap();
            let val = f64_type.const_float(*f);
            builder
                .build_call(printf_fn, &[fmt.as_pointer_value().into(), val.into()], "")
                .unwrap();
        }

        // print(True) / print(False)
        [Expr::Bool(b)] => {
            let s = if *b { "True" } else { "False" };
            let name = format!("str_{}", str_counter);
            *str_counter += 1;
            let global = builder.build_global_string_ptr(s, &name).unwrap();
            builder.build_call(puts_fn, &[global.as_pointer_value().into()], "").unwrap();
        }

        // Anything else: build a space-separated printf format string at compile time
        // for the common case where all args are known literals.
        multiple => {
            let (fmt_str, llvm_args) = build_format_string(multiple, context, i64_type, f64_type)?;
            let name = format!("str_{}", str_counter);
            *str_counter += 1;
            let fmt_global = builder.build_global_string_ptr(&fmt_str, &name).unwrap();
            let mut call_args: Vec<inkwell::values::BasicMetadataValueEnum> =
                vec![fmt_global.as_pointer_value().into()];
            call_args.extend(llvm_args);
            builder.build_call(printf_fn, &call_args, "").unwrap();
        }
    }

    Ok(())
}

// Build a printf format string from a list of literal args.
// Returns (format_string, [llvm values for each non-string arg]).
fn build_format_string<'ctx>(
    args: &[Expr],
    context: &'ctx Context,
    i64_type: inkwell::types::IntType<'ctx>,
    f64_type: inkwell::types::FloatType<'ctx>,
) -> Result<(String, Vec<inkwell::values::BasicMetadataValueEnum<'ctx>>), String> {
    let mut fmt = String::new();
    let mut llvm_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> = Vec::new();

    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            fmt.push(' ');
        }
        match arg {
            Expr::Str(s) => fmt.push_str(s),
            Expr::Int(n) => {
                fmt.push_str("%lld");
                llvm_args.push(i64_type.const_int(*n as u64, true).into());
            }
            Expr::Float(f) => {
                fmt.push_str("%g");
                llvm_args.push(f64_type.const_float(*f).into());
            }
            Expr::Bool(b) => fmt.push_str(if *b { "True" } else { "False" }),
            other => {
                return Err(format!(
                    "print() argument not yet supported by LLVM backend: {:?}",
                    other
                ))
            }
        }
    }
    fmt.push('\n');
    Ok((fmt, llvm_args))
}
