// cool-lang/src/llvm_codegen.rs
//
// LLVM backend for Cool.
//
// Architecture:
//   1. Embedded C runtime (RUNTIME_C const) defines CoolVal and all operations.
//   2. The Compiler emits LLVM IR that calls those C functions.
//   3. compile_program() writes the runtime to /tmp, compiles it with `cc`,
//      emits the LLVM module to a .o file, then links both together.

use crate::ast::{BinOp, ExceptHandler, Expr, FStringPart, Program, Stmt, UnaryOp};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::types::StructType;
use inkwell::values::{BasicMetadataValueEnum, BasicValue, FunctionValue, IntValue, PointerValue, StructValue};
use inkwell::{AddressSpace, InlineAsmDialect, IntPredicate, OptimizationLevel};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Embedded C runtime ────────────────────────────────────────────────────────

const RUNTIME_C: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <ctype.h>
#include <dirent.h>
#include <unistd.h>
#include <sys/stat.h>
#include <errno.h>
#include <time.h>
#include <regex.h>
#include <signal.h>
#include <sys/select.h>
#include <sys/wait.h>
#include <fcntl.h>
#include <dlfcn.h>
#include <setjmp.h>
#ifdef __APPLE__
#include <crt_externs.h>
#endif

#define TAG_NIL    0
#define TAG_INT    1
#define TAG_FLOAT  2
#define TAG_BOOL   3
#define TAG_STR    4
#define TAG_LIST   5
#define TAG_OBJECT 6
#define TAG_CLASS  7
#define TAG_DICT   8
#define TAG_TUPLE  9
#define TAG_CLOSURE 10
#define TAG_EXCEPTION 11
#define TAG_FILE  12
#define TAG_FFI_LIB 13
#define TAG_FFI_FUNC 14

/* The universal Cool value.
   Layout: { int32_t tag; [4 bytes pad]; int64_t payload }  = 16 bytes.
   Matches LLVM  %CoolVal = type { i32, i64 }               = 16 bytes.
   Floats are stored as their raw bit-pattern via memcpy.
   Strings are stored as (intptr_t)char* cast to int64_t.  */
typedef struct {
    int32_t tag;
    int64_t payload;
} CoolVal;

/* Forward declaration for cv_nil (needed by hashmap functions) */
CoolVal cv_nil(void);

/* Forward declaration for list */
typedef struct {
    int32_t tag;
    int64_t length;
    int64_t capacity;
    void* data;
} CoolList;

/* ── Dict (CoolVal-keyed hashmap) ───────────────────────────────────── */
typedef struct {
    int32_t tag;
    int64_t len;
    int64_t cap;
    CoolVal* keys;
    CoolVal* vals;
} CoolDict;

/* ── Simple hashmap for object attributes ───────────────────────────── */
typedef struct AttrNode {
    int64_t key;
    CoolVal value;
    struct AttrNode* next;
} AttrNode;

typedef struct {
    AttrNode** buckets;
    int64_t count;
    int64_t capacity;
} AttrMap;

static AttrMap* attrmap_create(void) {
    AttrMap* m = (AttrMap*)malloc(sizeof(AttrMap));
    if (!m) return NULL;
    m->capacity = 16;
    m->count = 0;
    m->buckets = (AttrNode**)calloc(16, sizeof(AttrNode*));
    return m;
}

static void attrmap_destroy(AttrMap* m) {
    if (!m) return;
    for (int64_t i = 0; i < m->capacity; i++) {
        AttrNode* n = m->buckets[i];
        while (n) { AttrNode* next = n->next; free(n); n = next; }
    }
    free(m->buckets);
    free(m);
}

/* Hash based on string content */
static int64_t attrmap_hash_str(const char* s, int64_t cap) {
    int64_t h = 0;
    while (*s) {
        h = h * 31 + *s;
        s++;
    }
    return (h & 0x7FFFFFFF) & (cap - 1);
}

/* Compare strings for equality */
static int attrmap_str_eq(int64_t a, int64_t b) {
    const char* sa = (const char*)(intptr_t)a;
    const char* sb = (const char*)(intptr_t)b;
    return strcmp(sa, sb) == 0;
}

static void attrmap_set(AttrMap* m, const char* key, CoolVal val) {
    if (!m) return;
    int64_t key_int = (int64_t)(intptr_t)key;
    int64_t idx = attrmap_hash_str(key, m->capacity);
    AttrNode* n = m->buckets[idx];
    while (n) {
        if (attrmap_str_eq(n->key, key_int)) { n->value = val; return; }
        n = n->next;
    }
    AttrNode* new_node = (AttrNode*)malloc(sizeof(AttrNode));
    if (!new_node) return;
    new_node->key = key_int;
    new_node->value = val;
    new_node->next = m->buckets[idx];
    m->buckets[idx] = new_node;
    m->count++;
}

static CoolVal attrmap_get(AttrMap* m, const char* key) {
    if (!m) return cv_nil();
    int64_t key_int = (int64_t)(intptr_t)key;
    int64_t idx = attrmap_hash_str(key, m->capacity);
    AttrNode* n = m->buckets[idx];
    while (n) {
        if (attrmap_str_eq(n->key, key_int)) return n->value;
        n = n->next;
    }
    return cv_nil();
}

/* ── Class definition ────────────────────────────────────────────────── */
typedef struct CoolClass {
    int32_t tag;       /* TAG_CLASS */
    int64_t name;      /* const char* */
    struct CoolClass* parent;
    int64_t method_count;
    /* Flexible array: [name1, ptr1, name2, ptr2, ...] (pairs) */
    int64_t methods[];  
} CoolClass;

/* ── Object (instance) ───────────────────────────────────────────────── */
typedef struct {
    int32_t tag;       /* TAG_OBJECT */
    CoolClass* class;
    AttrMap* attrs;
} CoolObject;

typedef struct {
    FILE* fp;
    int closed;
} CoolFile;

typedef struct {
    int32_t tag;
    void* handle;
} CoolFfiLib;

typedef struct {
    int32_t tag;
    void* handle;
    void* sym;
    char* name;
    int32_t ret_type;
    int32_t argc;
    int32_t arg_types[8];
} CoolFfiFunc;

typedef struct {
    int has_code;
    int code;
    int timed_out;
    char* stdout_data;
    char* stderr_data;
} CoolSubprocessResult;

/* Forward declarations for runtime functions */
CoolVal cv_nil(void);
CoolVal cv_int(int64_t);
CoolVal cv_float(double);
CoolVal cv_bool(int32_t);
CoolVal cv_str(const char*);
int32_t cool_truthy(CoolVal);
CoolVal cool_add(CoolVal, CoolVal);
CoolVal cool_sub(CoolVal, CoolVal);
CoolVal cool_mul(CoolVal, CoolVal);
CoolVal cool_div(CoolVal, CoolVal);
CoolVal cool_mod(CoolVal, CoolVal);
CoolVal cool_pow(CoolVal, CoolVal);
CoolVal cool_floordiv(CoolVal, CoolVal);
CoolVal cool_neg(CoolVal);
CoolVal cool_eq(CoolVal, CoolVal);
CoolVal cool_neq(CoolVal, CoolVal);
CoolVal cool_lt(CoolVal, CoolVal);
CoolVal cool_lteq(CoolVal, CoolVal);
CoolVal cool_gt(CoolVal, CoolVal);
CoolVal cool_gteq(CoolVal, CoolVal);
CoolVal cool_not(CoolVal);
CoolVal cool_bitand(CoolVal, CoolVal);
CoolVal cool_bitor(CoolVal, CoolVal);
CoolVal cool_bitxor(CoolVal, CoolVal);
CoolVal cool_bitnot(CoolVal);
CoolVal cool_lshift(CoolVal, CoolVal);
CoolVal cool_rshift(CoolVal, CoolVal);
CoolVal cool_list_make(CoolVal);
CoolVal cool_tuple_make(CoolVal);
CoolVal cool_list_len(CoolVal);
CoolVal cool_type(CoolVal);
CoolVal cool_list_get(CoolVal, CoolVal);
CoolVal cool_list_set(CoolVal, CoolVal, CoolVal);
CoolVal cool_list_push(CoolVal, CoolVal);
CoolVal cool_list_concat(CoolVal, CoolVal);
CoolVal cool_dict_new(void);
CoolVal cool_dict_get(CoolVal, CoolVal);
CoolVal cool_dict_set(CoolVal, CoolVal, CoolVal);
CoolVal cool_dict_len(CoolVal);
CoolVal cool_dict_contains(CoolVal, CoolVal);
CoolVal cool_dict_get_opt(CoolVal, CoolVal);
CoolVal cool_index(CoolVal, CoolVal);
CoolVal cool_slice(CoolVal, CoolVal, CoolVal);
CoolVal cool_setindex(CoolVal, CoolVal, CoolVal);
CoolVal cool_file_open(CoolVal, CoolVal);
CoolVal cool_abs(CoolVal);
CoolVal cool_to_int(CoolVal);
CoolVal cool_to_float_val(CoolVal);
CoolVal cool_to_bool_val(CoolVal);
CoolVal cool_module_get_attr(const char*, const char*);
CoolVal cool_module_call(const char*, const char*, int32_t, ...);
CoolVal cool_noncallable(CoolVal);
CoolVal cool_ffi_open(CoolVal);
CoolVal cool_ffi_func(CoolVal, CoolVal, CoolVal, CoolVal);
CoolVal cool_ffi_call(CoolVal, int32_t, ...);
int32_t cool_is_ffi_func(CoolVal);
CoolVal cool_round(CoolVal, CoolVal);
CoolVal cool_sorted(CoolVal);
CoolVal cool_sum(CoolVal);
int64_t cool_closure_get_fn_ptr(CoolVal);
int32_t cool_is_closure(CoolVal);
void cool_enter_try(void*);
void cool_exit_try(void);
CoolVal cool_get_exception(void);
void cool_raise(CoolVal);
void cool_register_class_parent(const char*, const char*);
void cool_push_with(CoolVal);
void cool_pop_with(void);
static CoolSubprocessResult cool_subprocess_run_shell(const char*, int, double);
static CoolVal cool_subprocess_result_dict(CoolSubprocessResult);
void cool_print(int32_t, ...);

/* ── class / object support ─────────────────────────────────────────── */
CoolVal cool_class_new(const char*, CoolVal, int64_t, int64_t*);
CoolVal cool_object_new(CoolVal);
CoolVal cool_get_attr(CoolVal, const char*);
CoolVal cool_set_attr(CoolVal, const char*, CoolVal);
CoolVal cool_call_method_vararg(CoolVal, const char*, int32_t, ...);
CoolVal cool_get_arg(int32_t);
CoolVal cool_is_instance(CoolVal, const char*);
int32_t cool_exception_matches(CoolVal, const char*);
int64_t cool_get_method_ptr(CoolVal, const char*);

/* ── bit-pattern helpers ──────────────────────────────────────────────── */
static double cv_as_float(CoolVal v) {
    double d;
    memcpy(&d, &v.payload, sizeof(double));
    return d;
}
static double cv_to_float(CoolVal v) {
    if (v.tag == TAG_FLOAT) return cv_as_float(v);
    if (v.tag == TAG_INT)   return (double)v.payload;
    return 0.0;
}

/* ── constructors ─────────────────────────────────────────────────────── */
CoolVal cv_nil(void)           { CoolVal v; v.tag = TAG_NIL;   v.payload = 0;                    return v; }
CoolVal cv_int(int64_t n)      { CoolVal v; v.tag = TAG_INT;   v.payload = n;                    return v; }
CoolVal cv_bool(int32_t b)     { CoolVal v; v.tag = TAG_BOOL;  v.payload = b ? 1 : 0;            return v; }
CoolVal cv_str(const char* s)  { CoolVal v; v.tag = TAG_STR;   v.payload = (int64_t)(intptr_t)s; return v; }
CoolVal cv_float(double f) {
    CoolVal v; v.tag = TAG_FLOAT;
    memcpy(&v.payload, &f, sizeof(double));
    return v;
}

/* ── truthiness ───────────────────────────────────────────────────────── */
int32_t cool_truthy(CoolVal v) {
    switch (v.tag) {
        case TAG_NIL:   return 0;
        case TAG_INT:   return v.payload != 0 ? 1 : 0;
        case TAG_FLOAT: return cv_as_float(v) != 0.0 ? 1 : 0;
        case TAG_BOOL:  return v.payload != 0 ? 1 : 0;
        case TAG_STR:   return ((const char*)(intptr_t)v.payload)[0] != '\0' ? 1 : 0;
        default:        return 0;
    }
}

typedef CoolVal (*CoolFn0)(void);
typedef CoolVal (*CoolFn1)(CoolVal);
typedef CoolVal (*CoolFn2)(CoolVal, CoolVal);
typedef CoolVal (*CoolFn3)(CoolVal, CoolVal, CoolVal);
typedef CoolVal (*CoolFn4)(CoolVal, CoolVal, CoolVal, CoolVal);
typedef CoolVal (*CoolFn5)(CoolVal, CoolVal, CoolVal, CoolVal, CoolVal);

static CoolVal call_cool_fn_ptr(int64_t fn_ptr, int32_t argc, CoolVal* argv) {
    switch (argc) {
        case 0: return ((CoolFn0)(intptr_t)fn_ptr)();
        case 1: return ((CoolFn1)(intptr_t)fn_ptr)(argv[0]);
        case 2: return ((CoolFn2)(intptr_t)fn_ptr)(argv[0], argv[1]);
        case 3: return ((CoolFn3)(intptr_t)fn_ptr)(argv[0], argv[1], argv[2]);
        case 4: return ((CoolFn4)(intptr_t)fn_ptr)(argv[0], argv[1], argv[2], argv[3]);
        case 5: return ((CoolFn5)(intptr_t)fn_ptr)(argv[0], argv[1], argv[2], argv[3], argv[4]);
        default:
            fprintf(stderr, "RuntimeError: too many arguments for native call (%d)\n", argc);
            exit(1);
    }
}

/* ── arithmetic ───────────────────────────────────────────────────────── */
CoolVal cool_add(CoolVal a, CoolVal b) {
    if (a.tag == TAG_OBJECT) {
        CoolObject* o = (CoolObject*)(intptr_t)a.payload;
        int64_t fn_ptr = o && o->class ? cool_get_method_ptr((CoolVal){TAG_CLASS, (int64_t)(intptr_t)o->class}, "method___add__") : 0;
        if (fn_ptr) {
            CoolVal argv[2] = {a, b};
            return call_cool_fn_ptr(fn_ptr, 2, argv);
        }
    }
    if (a.tag == TAG_STR && b.tag == TAG_STR) {
        const char* sa = (const char*)(intptr_t)a.payload;
        const char* sb = (const char*)(intptr_t)b.payload;
        size_t la = strlen(sa), lb = strlen(sb);
        char* r = (char*)malloc(la + lb + 1);
        memcpy(r, sa, la); memcpy(r + la, sb, lb); r[la + lb] = '\0';
        return cv_str(r);
    }
    if (a.tag == TAG_LIST && b.tag == TAG_LIST)
        return cool_list_concat(a, b);
    if (a.tag == TAG_FLOAT || b.tag == TAG_FLOAT)
        return cv_float(cv_to_float(a) + cv_to_float(b));
    return cv_int(a.payload + b.payload);
}
CoolVal cool_sub(CoolVal a, CoolVal b) {
    if (a.tag == TAG_FLOAT || b.tag == TAG_FLOAT) return cv_float(cv_to_float(a) - cv_to_float(b));
    return cv_int(a.payload - b.payload);
}
CoolVal cool_mul(CoolVal a, CoolVal b) {
    if (a.tag == TAG_FLOAT || b.tag == TAG_FLOAT) return cv_float(cv_to_float(a) * cv_to_float(b));
    return cv_int(a.payload * b.payload);
}
CoolVal cool_div(CoolVal a, CoolVal b) {
    return cv_float(cv_to_float(a) / cv_to_float(b));
}
CoolVal cool_mod(CoolVal a, CoolVal b) {
    if (a.tag == TAG_INT && b.tag == TAG_INT) {
        if (b.payload == 0) { fputs("ZeroDivisionError\n", stderr); exit(1); }
        int64_t r = a.payload % b.payload;
        if (r != 0 && ((r < 0) != (b.payload < 0))) r += b.payload;
        return cv_int(r);
    }
    double fa = cv_to_float(a), fb = cv_to_float(b);
    double r = fmod(fa, fb);
    if (r != 0.0 && ((r < 0.0) != (fb < 0.0))) r += fb;
    return cv_float(r);
}
CoolVal cool_pow(CoolVal a, CoolVal b) {
    return cv_float(pow(cv_to_float(a), cv_to_float(b)));
}
CoolVal cool_floordiv(CoolVal a, CoolVal b) {
    if (a.tag == TAG_INT && b.tag == TAG_INT) {
        if (b.payload == 0) { fputs("ZeroDivisionError\n", stderr); exit(1); }
        int64_t q = a.payload / b.payload;
        if ((a.payload ^ b.payload) < 0 && q * b.payload != a.payload) q--;
        return cv_int(q);
    }
    return cv_float(floor(cv_to_float(a) / cv_to_float(b)));
}
CoolVal cool_neg(CoolVal a) {
    if (a.tag == TAG_FLOAT) return cv_float(-cv_as_float(a));
    return cv_int(-a.payload);
}

/* ── comparisons ──────────────────────────────────────────────────────── */
static int cv_eq_raw(CoolVal a, CoolVal b) {
    if (a.tag == TAG_OBJECT) {
        CoolObject* o = (CoolObject*)(intptr_t)a.payload;
        int64_t fn_ptr = o && o->class ? cool_get_method_ptr((CoolVal){TAG_CLASS, (int64_t)(intptr_t)o->class}, "method___eq__") : 0;
        if (fn_ptr) {
            CoolVal argv[2] = {a, b};
            return cool_truthy(call_cool_fn_ptr(fn_ptr, 2, argv));
        }
    }
    if (a.tag != b.tag) {
        int an = a.tag == TAG_INT || a.tag == TAG_FLOAT;
        int bn = b.tag == TAG_INT || b.tag == TAG_FLOAT;
        if (an && bn) return cv_to_float(a) == cv_to_float(b);
        return 0;
    }
    switch (a.tag) {
        case TAG_NIL:   return 1;
        case TAG_INT:   return a.payload == b.payload;
        case TAG_FLOAT: return cv_as_float(a) == cv_as_float(b);
        case TAG_BOOL:  return a.payload == b.payload;
        case TAG_STR:   return strcmp((const char*)(intptr_t)a.payload,
                                      (const char*)(intptr_t)b.payload) == 0;
        default:        return 0;
    }
}
CoolVal cool_eq(CoolVal a, CoolVal b)   { return cv_bool(cv_eq_raw(a,b)); }
CoolVal cool_neq(CoolVal a, CoolVal b)  { return cv_bool(!cv_eq_raw(a,b)); }

#define STR_CMP(op) \
    if (a.tag == TAG_STR && b.tag == TAG_STR) \
        return cv_bool(strcmp((const char*)(intptr_t)a.payload, \
                              (const char*)(intptr_t)b.payload) op 0); \
    return cv_bool(cv_to_float(a) op cv_to_float(b))

CoolVal cool_lt(CoolVal a, CoolVal b)   { 
    if (a.tag == TAG_INT && b.tag == TAG_INT) return cv_bool(a.payload < b.payload);
    if (a.tag == TAG_STR && b.tag == TAG_STR) return cv_bool(strcmp((const char*)(intptr_t)a.payload, (const char*)(intptr_t)b.payload) < 0);
    return cv_bool(cv_to_float(a) < cv_to_float(b));
}
CoolVal cool_lteq(CoolVal a, CoolVal b) { 
    if (a.tag == TAG_INT && b.tag == TAG_INT) return cv_bool(a.payload <= b.payload);
    if (a.tag == TAG_STR && b.tag == TAG_STR) return cv_bool(strcmp((const char*)(intptr_t)a.payload, (const char*)(intptr_t)b.payload) <= 0);
    return cv_bool(cv_to_float(a) <= cv_to_float(b));
}
CoolVal cool_gt(CoolVal a, CoolVal b)   { 
    if (a.tag == TAG_INT && b.tag == TAG_INT) return cv_bool(a.payload > b.payload);
    if (a.tag == TAG_STR && b.tag == TAG_STR) return cv_bool(strcmp((const char*)(intptr_t)a.payload, (const char*)(intptr_t)b.payload) > 0);
    return cv_bool(cv_to_float(a) > cv_to_float(b));
}
CoolVal cool_gteq(CoolVal a, CoolVal b) { 
    if (a.tag == TAG_INT && b.tag == TAG_INT) return cv_bool(a.payload >= b.payload);
    if (a.tag == TAG_STR && b.tag == TAG_STR) return cv_bool(strcmp((const char*)(intptr_t)a.payload, (const char*)(intptr_t)b.payload) >= 0);
    return cv_bool(cv_to_float(a) >= cv_to_float(b));
}

/* ── logic / bitwise ──────────────────────────────────────────────────── */
CoolVal cool_not(CoolVal a)              { return cv_bool(!cool_truthy(a)); }
CoolVal cool_bitand(CoolVal a, CoolVal b){ return cv_int((int64_t)a.payload & (int64_t)b.payload); }
CoolVal cool_bitor(CoolVal a, CoolVal b) { return cv_int((int64_t)a.payload | (int64_t)b.payload); }
CoolVal cool_bitxor(CoolVal a, CoolVal b){ return cv_int((int64_t)a.payload ^ (int64_t)b.payload); }
CoolVal cool_bitnot(CoolVal a)           { return cv_int(~(int64_t)a.payload); }
CoolVal cool_lshift(CoolVal a, CoolVal b){ return cv_int((int64_t)a.payload << (int)b.payload); }
CoolVal cool_rshift(CoolVal a, CoolVal b){ return cv_int((int64_t)a.payload >> (int)b.payload); }

/* ── list operations ─────────────────────────────────────────────────────── */
CoolVal cool_list_make(CoolVal n_val) {
    int64_t n = n_val.payload;
    /* LIST MAKE: create empty list, capacity = n */
    CoolList* lst = (CoolList*)malloc(sizeof(CoolList));
    if (!lst) return cv_nil();
    lst->tag = TAG_LIST;
    lst->length = 0;
    lst->capacity = n > 0 ? n : 1;
    lst->data = malloc(lst->capacity * sizeof(CoolVal));
    if (!lst->data) {
        free(lst);
        return cv_nil();
    }
    CoolVal v;
    v.tag = TAG_LIST;
    v.payload = (int64_t)(intptr_t)lst;
    return v;
}

CoolVal cool_tuple_make(CoolVal n_val) {
    int64_t n = n_val.payload;
    CoolList* lst = (CoolList*)malloc(sizeof(CoolList));
    if (!lst) return cv_nil();
    lst->tag = TAG_TUPLE;
    lst->length = 0;
    lst->capacity = n > 0 ? n : 1;
    lst->data = malloc(lst->capacity * sizeof(CoolVal));
    if (!lst->data) { free(lst); return cv_nil(); }
    CoolVal v; v.tag = TAG_TUPLE; v.payload = (int64_t)(intptr_t)lst;
    return v;
}

/* ── to_str ─────────���─────────────────────────────────────────────────── */
char* cool_to_str(CoolVal v) {
    if (v.tag == TAG_STR) return (char*)(intptr_t)v.payload;
    if (v.tag == TAG_OBJECT) {
        CoolObject* o = (CoolObject*)(intptr_t)v.payload;
        int64_t fn_ptr = o && o->class ? cool_get_method_ptr((CoolVal){TAG_CLASS, (int64_t)(intptr_t)o->class}, "method___str__") : 0;
        if (fn_ptr) {
            CoolVal argv[1] = {v};
            CoolVal res = call_cool_fn_ptr(fn_ptr, 1, argv);
            if (res.tag == TAG_STR) return (char*)(intptr_t)res.payload;
        }
    }
    char* buf = (char*)malloc(64);
    if (!buf) return (char*)"<oom>";
    switch (v.tag) {
        case TAG_NIL:   snprintf(buf, 64, "nil");                             break;
        case TAG_INT:   snprintf(buf, 64, "%lld", (long long)v.payload);      break;
        case TAG_FLOAT: snprintf(buf, 64, "%g",   cv_as_float(v));            break;
        case TAG_BOOL:  snprintf(buf, 64, "%s",   v.payload ? "true":"false"); break;
        case TAG_FILE:  snprintf(buf, 64, "<file>");                          break;
        case TAG_FFI_LIB: snprintf(buf, 64, "<ffi library>");                break;
        case TAG_FFI_FUNC: {
            CoolFfiFunc* fn = (CoolFfiFunc*)(intptr_t)v.payload;
            snprintf(buf, 64, "<ffi func %s>", fn && fn->name ? fn->name : "?");
            break;
        }
        case TAG_LIST: {
            CoolList* lst = (CoolList*)(intptr_t)v.payload;
            if (!lst || !lst->data) { snprintf(buf, 64, "[]"); break; }
            char* p = buf;
            *p++ = '[';
            for (int64_t i = 0; i < lst->length; i++) {
                if (i > 0) *p++ = ',';
                char* elem = cool_to_str(((CoolVal*)lst->data)[i]);
                size_t len = strlen(elem);
                if (p - buf + len > 62) { *p++ = '.'; *p++ = '.'; *p++ = '.'; break; }
                memcpy(p, elem, len);
                p += len;
            }
            *p++ = ']';
            *p = '\0';
            break;
        }
        default:        snprintf(buf, 64, "<unknown>");                        break;
    }
    return buf;
}

/* ── raw memory ───────────────────────────────────────────────────────── */
CoolVal cool_malloc(CoolVal size_val) {
    size_t n = (size_t)(uintptr_t)size_val.payload;
    void* p = malloc(n);
    return cv_int((int64_t)(intptr_t)p);
}
CoolVal cool_free(CoolVal ptr_val) {
    free((void*)(intptr_t)ptr_val.payload);
    return cv_nil();
}
CoolVal cool_read_byte(CoolVal addr_val) {
    uint8_t* p = (uint8_t*)(intptr_t)addr_val.payload;
    return cv_int((int64_t)*p);
}
CoolVal cool_write_byte(CoolVal addr_val, CoolVal val) {
    uint8_t* p = (uint8_t*)(intptr_t)addr_val.payload;
    *p = (uint8_t)val.payload;
    return cv_nil();
}
CoolVal cool_read_i64(CoolVal addr_val) {
    int64_t* p = (int64_t*)(intptr_t)addr_val.payload;
    return cv_int(*p);
}
CoolVal cool_write_i64(CoolVal addr_val, CoolVal val) {
    int64_t* p = (int64_t*)(intptr_t)addr_val.payload;
    *p = val.payload;
    return cv_nil();
}
CoolVal cool_read_f64(CoolVal addr_val) {
    double* p = (double*)(intptr_t)addr_val.payload;
    return cv_float(*p);
}
CoolVal cool_write_f64(CoolVal addr_val, CoolVal val) {
    double* p = (double*)(intptr_t)addr_val.payload;
    *p = cv_to_float(val);
    return cv_nil();
}
CoolVal cool_read_str(CoolVal addr_val) {
    char* p = (char*)(intptr_t)addr_val.payload;
    return cv_str(p);
}
CoolVal cool_write_str(CoolVal addr_val, CoolVal str_val) {
    char* dst = (char*)(intptr_t)addr_val.payload;
    const char* src = (const char*)(intptr_t)str_val.payload;
    strcpy(dst, src);
    return cv_nil();
}

/* ── list operations (continued) ───────────────────────────────────────── */
CoolVal cool_list_len(CoolVal v) {
    if (v.tag != TAG_LIST) return cv_int(0);
    CoolList* lst = (CoolList*)(intptr_t)v.payload;
    return cv_int(lst->length);
}

const char* cool_type_name(int32_t tag) {
    switch(tag) {
        case TAG_NIL:    return "nil";
        case TAG_INT:    return "int";
        case TAG_FLOAT:  return "float";
        case TAG_BOOL:   return "bool";
        case TAG_STR:    return "str";
        case TAG_LIST:   return "list";
        case TAG_DICT:   return "dict";
        case TAG_OBJECT: return "object";
        case TAG_TUPLE:  return "tuple";
        case TAG_FILE:   return "file";
        case TAG_FFI_LIB: return "ffi_lib";
        case TAG_FFI_FUNC: return "ffi_func";
        default:         return "unknown";
    }
}

CoolVal cool_type(CoolVal v) {
    if (v.tag == TAG_OBJECT) {
        CoolObject* o = (CoolObject*)(intptr_t)v.payload;
        if (o && o->class) return cv_str((const char*)(intptr_t)o->class->name);
    }
    return cv_str(cool_type_name(v.tag));
}

CoolVal cool_list_get(CoolVal list_val, CoolVal idx_val) {
    if (list_val.tag != TAG_LIST && list_val.tag != TAG_TUPLE) return cv_nil();
    int64_t idx = idx_val.payload;
    CoolList* lst = (CoolList*)(intptr_t)list_val.payload;
    if (idx < 0) idx += lst->length;
    if (idx < 0 || idx >= lst->length) return cv_nil();
    return ((CoolVal*)lst->data)[idx];
}
CoolVal cool_list_set(CoolVal list_val, CoolVal idx_val, CoolVal val) {
    if (list_val.tag != TAG_LIST) return cv_nil();
    int64_t idx = idx_val.payload;
    CoolList* lst = (CoolList*)(intptr_t)list_val.payload;
    if (idx < 0) idx += lst->length;
    if (idx < 0 || idx >= lst->length) return cv_nil();
    ((CoolVal*)lst->data)[idx] = val;
    return cv_nil();
}
CoolVal cool_list_push(CoolVal list_val, CoolVal val) {
    if (list_val.tag != TAG_LIST && list_val.tag != TAG_TUPLE) return cv_nil();
    CoolList* lst = (CoolList*)(intptr_t)list_val.payload;
    if (lst->length >= lst->capacity) {
        int64_t new_cap = lst->capacity * 2;
        void* new_data = realloc(lst->data, new_cap * sizeof(CoolVal));
        if (!new_data) return cv_nil();
        lst->data = new_data;
        lst->capacity = new_cap;
    }
    ((CoolVal*)lst->data)[lst->length++] = val;
    return cv_nil();
}
CoolVal cool_list_pop(CoolVal list_val) {
    if (list_val.tag != TAG_LIST) return cv_nil();
    CoolList* lst = (CoolList*)(intptr_t)list_val.payload;
    if (lst->length <= 0) return cv_nil();
    return ((CoolVal*)lst->data)[--lst->length];
}
/* ── len() ──────────────────────────────────────────────────────────────── */
CoolVal cool_len(CoolVal v) {
    switch (v.tag) {
        case TAG_STR: return cv_int(strlen((const char*)(intptr_t)v.payload));
        case TAG_LIST:
        case TAG_TUPLE: {
            CoolList* lst = (CoolList*)(intptr_t)v.payload;
            return cv_int(lst->length);
        }
        case TAG_DICT: {
            CoolDict* d = (CoolDict*)(intptr_t)v.payload;
            return cv_int(d->len);
        }
        case TAG_OBJECT: {
            CoolObject* o = (CoolObject*)(intptr_t)v.payload;
            int64_t fn_ptr = o && o->class ? cool_get_method_ptr((CoolVal){TAG_CLASS, (int64_t)(intptr_t)o->class}, "method___len__") : 0;
            if (fn_ptr) {
                CoolVal argv[1] = {v};
                return call_cool_fn_ptr(fn_ptr, 1, argv);
            }
            return cv_int(0);
        }
        default: return cv_int(0);
    }
}

/* ── range() ──────────────────────────────────────────────────────────────── */
/* RANGE: create list from start to stop with step */
CoolVal cool_range(CoolVal start_val, CoolVal stop_val, CoolVal step_val) {
    int64_t start = start_val.payload;
    int64_t stop = stop_val.payload;
    int64_t step = step_val.payload;
    if (step == 0) step = 1;
    int64_t n = 0;
    if (step > 0) {
        for (int64_t i = start; i < stop; i += step) n++;
    } else {
        for (int64_t i = start; i > stop; i += step) n++;
    }
    CoolList* lst = (CoolList*)malloc(sizeof(CoolList));
    if (!lst) return cv_nil();
    lst->tag = TAG_LIST;
    lst->length = 0;
    lst->capacity = n > 0 ? n : 1;
    lst->data = malloc(n * sizeof(CoolVal));
    if (!lst->data) { free(lst); return cv_nil(); }
    for (int64_t i = start; step > 0 ? i < stop : i > stop; i += step) {
        ((CoolVal*)lst->data)[lst->length++] = cv_int(i);
    }
    CoolVal v;
    v.tag = TAG_LIST;
    v.payload = (int64_t)(intptr_t)lst;
    return v;
}

/* ── list concatenation ───────────────────────────────────────────────── */
CoolVal cool_list_concat(CoolVal a, CoolVal b) {
    if (a.tag != TAG_LIST || b.tag != TAG_LIST) return cv_nil();
    CoolList* la = (CoolList*)(intptr_t)a.payload;
    CoolList* lb = (CoolList*)(intptr_t)b.payload;
    int64_t n = la->length + lb->length;
    CoolList* r = (CoolList*)malloc(sizeof(CoolList));
    if (!r) return cv_nil();
    r->tag = TAG_LIST;
    r->length = 0;
    r->capacity = n > 0 ? n : 1;
    r->data = malloc(n * sizeof(CoolVal));
    if (!r->data) { free(r); return cv_nil(); }
    for (int64_t i = 0; i < la->length; i++) {
        ((CoolVal*)r->data)[r->length++] = ((CoolVal*)la->data)[i];
    }
    for (int64_t i = 0; i < lb->length; i++) {
        ((CoolVal*)r->data)[r->length++] = ((CoolVal*)lb->data)[i];
    }
    CoolVal v;
    v.tag = TAG_LIST;
    v.payload = (int64_t)(intptr_t)r;
    return v;
}

/* ── print ────────────────────────────────────────────────────────────── */
void cool_print(int32_t n, ...) {
    va_list ap;
    va_start(ap, n);
    for (int32_t i = 0; i < n; i++) {
        if (i > 0) putchar(' ');
        CoolVal v = va_arg(ap, CoolVal);
        switch (v.tag) {
            case TAG_NIL:   fputs("nil",  stdout); break;
            case TAG_INT:   printf("%lld", (long long)v.payload); break;
            case TAG_FLOAT: printf("%g",   cv_as_float(v));       break;
            case TAG_BOOL:  fputs(v.payload ? "true" : "false", stdout); break;
            case TAG_STR:   fputs((const char*)(intptr_t)v.payload, stdout); break;
            case TAG_FILE:  fputs("<file>", stdout); break;
            case TAG_LIST: {
                CoolList* lst = (CoolList*)(intptr_t)v.payload;
                if (!lst || !lst->data) { fputs("[]", stdout); break; }
                putchar('[');
                for (int64_t i = 0; i < lst->length; i++) {
                    if (i > 0) { putchar(','); putchar(' '); }
                    char* elem = cool_to_str(((CoolVal*)lst->data)[i]);
                    fputs(elem, stdout);
                }
                putchar(']');
                break;
            }
            default:        fputs("<unknown>", stdout); break;
        }
    }
    va_end(ap);
    putchar('\n');
}

/* ── class operations ─────────────────────────────────────────────────── */
typedef struct {
    const char* class_name;
    const char* parent_name;
} CoolClassParent;
#define MAX_CLASS_PARENTS 256
static CoolClassParent g_class_parents[MAX_CLASS_PARENTS];
static int g_class_parent_count = 0;

static const char* cool_lookup_parent_name(const char* class_name) {
    for (int i = g_class_parent_count - 1; i >= 0; i--) {
        if (strcmp(g_class_parents[i].class_name, class_name) == 0) {
            return g_class_parents[i].parent_name;
        }
    }
    return NULL;
}

void cool_register_class_parent(const char* class_name, const char* parent_name) {
    if (!class_name || !parent_name || !*parent_name) return;
    for (int i = 0; i < g_class_parent_count; i++) {
        if (strcmp(g_class_parents[i].class_name, class_name) == 0) {
            g_class_parents[i].parent_name = parent_name;
            return;
        }
    }
    if (g_class_parent_count < MAX_CLASS_PARENTS) {
        g_class_parents[g_class_parent_count].class_name = class_name;
        g_class_parents[g_class_parent_count].parent_name = parent_name;
        g_class_parent_count++;
    }
}

static int cool_class_name_matches(const char* actual_name, const char* expected_name) {
    const char* cur = actual_name;
    int depth = 0;
    while (cur && depth++ < MAX_CLASS_PARENTS) {
        if (strcmp(cur, expected_name) == 0) {
            return 1;
        }
        cur = cool_lookup_parent_name(cur);
    }
    return 0;
}

CoolVal cool_class_new(const char* name, CoolVal parent_val, int64_t method_count, int64_t* method_ptrs) {
    CoolClass* cls = (CoolClass*)malloc(sizeof(CoolClass) + 2 * method_count * sizeof(int64_t));
    if (!cls) return cv_nil();
    cls->tag = TAG_CLASS;
    cls->name = (int64_t)(intptr_t)name;
    cls->parent = parent_val.tag == TAG_CLASS ? (CoolClass*)(intptr_t)parent_val.payload : NULL;
    cls->method_count = method_count;
    for (int64_t i = 0; i < method_count; i++) {
        cls->methods[i * 2] = method_ptrs[i * 2];     /* name pointer */
        cls->methods[i * 2 + 1] = method_ptrs[i * 2 + 1]; /* function pointer */
    }
    CoolVal v;
    v.tag = TAG_CLASS;
    v.payload = (int64_t)(intptr_t)cls;
    return v;
}

CoolVal cool_object_new(CoolVal class_val) {
    if (class_val.tag != TAG_CLASS) return cv_nil();
    CoolClass* cls = (CoolClass*)(intptr_t)class_val.payload;
    CoolObject* obj = (CoolObject*)malloc(sizeof(CoolObject));
    if (!obj) return cv_nil();
    obj->tag = TAG_OBJECT;
    obj->class = cls;
    obj->attrs = attrmap_create();
    CoolVal v;
    v.tag = TAG_OBJECT;
    v.payload = (int64_t)(intptr_t)obj;
    return v;
}

CoolVal cool_get_attr(CoolVal obj, const char* name) {
    if (obj.tag == TAG_DICT) return cool_dict_get_opt(obj, cv_str(name));
    if (obj.tag != TAG_OBJECT) return cv_nil();
    CoolObject* o = (CoolObject*)(intptr_t)obj.payload;
    if (!o->attrs) return cv_nil();
    return attrmap_get(o->attrs, name);
}

CoolVal cool_set_attr(CoolVal obj, const char* name, CoolVal value) {
    if (obj.tag == TAG_DICT) return cool_dict_set(obj, cv_str(name), value);
    if (obj.tag != TAG_OBJECT) return cv_nil();
    CoolObject* o = (CoolObject*)(intptr_t)obj.payload;
    if (!o->attrs) return cv_nil();
    attrmap_set(o->attrs, name, value);
    return value;
}

int64_t cool_get_method_ptr(CoolVal class_val, const char* name) {
    if (class_val.tag != TAG_CLASS) return 0;
    CoolClass* cls = (CoolClass*)(intptr_t)class_val.payload;
    for (int64_t i = 0; i < cls->method_count; i++) {
        const char* mname = (const char*)(intptr_t)cls->methods[i * 2];
        if (mname && strcmp(mname, name) == 0) {
            return cls->methods[i * 2 + 1];
        }
    }
    return 0;
}

/* Global for passing method arguments */
static CoolVal g_method_args[32];
static int g_method_arg_count = 0;

typedef struct CoolStrBuf {
    char* data;
    size_t len;
    size_t cap;
} CoolStrBuf;
static CoolVal cool_list_contains_local(CoolVal list, CoolVal item);
static void sb_init(CoolStrBuf* sb);
static void sb_push_char(CoolStrBuf* sb, char c);
static void sb_push_str(CoolStrBuf* sb, const char* s);
static int cool_mkdir_p(const char* path);
static char* re_translate_pattern(const char* pattern);
static regex_t re_compile_regex(const char* pattern);

static CoolVal cool_string_upper(CoolVal obj) {
    const char* s = (const char*)(intptr_t)obj.payload;
    size_t len = strlen(s);
    char* out = (char*)malloc(len + 1);
    for (size_t i = 0; i < len; i++) out[i] = (char)toupper((unsigned char)s[i]);
    out[len] = '\0';
    return cv_str(out);
}

static CoolVal cool_string_lower(CoolVal obj) {
    const char* s = (const char*)(intptr_t)obj.payload;
    size_t len = strlen(s);
    char* out = (char*)malloc(len + 1);
    for (size_t i = 0; i < len; i++) out[i] = (char)tolower((unsigned char)s[i]);
    out[len] = '\0';
    return cv_str(out);
}

static CoolVal cool_string_strip(CoolVal obj) {
    const char* s = (const char*)(intptr_t)obj.payload;
    const char* start = s;
    while (*start && isspace((unsigned char)*start)) start++;
    const char* end = s + strlen(s);
    while (end > start && isspace((unsigned char)end[-1])) end--;
    size_t len = (size_t)(end - start);
    char* out = (char*)malloc(len + 1);
    memcpy(out, start, len);
    out[len] = '\0';
    return cv_str(out);
}

static CoolVal cool_string_lstrip(CoolVal obj) {
    const char* s = (const char*)(intptr_t)obj.payload;
    while (*s && isspace((unsigned char)*s)) s++;
    return cv_str(strdup(s));
}

static CoolVal cool_string_rstrip(CoolVal obj) {
    const char* s = (const char*)(intptr_t)obj.payload;
    size_t len = strlen(s);
    while (len > 0 && isspace((unsigned char)s[len - 1])) len--;
    char* out = (char*)malloc(len + 1);
    memcpy(out, s, len);
    out[len] = '\0';
    return cv_str(out);
}

static CoolVal cool_string_join(CoolVal sep, CoolVal seq) {
    if (seq.tag != TAG_LIST && seq.tag != TAG_TUPLE) return cv_nil();
    const char* delim = (const char*)(intptr_t)sep.payload;
    CoolList* lst = (CoolList*)(intptr_t)seq.payload;
    size_t delim_len = strlen(delim);
    size_t total = 1;
    for (int64_t i = 0; i < lst->length; i++) {
        total += strlen(cool_to_str(((CoolVal*)lst->data)[i]));
        if (i > 0) total += delim_len;
    }
    char* out = (char*)malloc(total);
    char* p = out;
    for (int64_t i = 0; i < lst->length; i++) {
        if (i > 0) {
            memcpy(p, delim, delim_len);
            p += delim_len;
        }
        char* elem = cool_to_str(((CoolVal*)lst->data)[i]);
        size_t len = strlen(elem);
        memcpy(p, elem, len);
        p += len;
    }
    *p = '\0';
    return cv_str(out);
}

static CoolVal cool_string_split(CoolVal obj, CoolVal sep_val) {
    const char* s = (const char*)(intptr_t)obj.payload;
    CoolVal out = cool_list_make(cv_int(4));
    if (sep_val.tag == TAG_NIL) {
        while (*s) {
            while (*s && isspace((unsigned char)*s)) s++;
            if (!*s) break;
            const char* start = s;
            while (*s && !isspace((unsigned char)*s)) s++;
            size_t len = (size_t)(s - start);
            char* part = (char*)malloc(len + 1);
            memcpy(part, start, len);
            part[len] = '\0';
            cool_list_push(out, cv_str(part));
        }
        return out;
    }
    const char* sep = (const char*)(intptr_t)sep_val.payload;
    size_t sep_len = strlen(sep);
    if (sep_len == 0) {
        while (*s) {
            char* part = (char*)malloc(2);
            part[0] = *s++;
            part[1] = '\0';
            cool_list_push(out, cv_str(part));
        }
        return out;
    }
    const char* start = s;
    const char* pos;
    while ((pos = strstr(start, sep)) != NULL) {
        size_t len = (size_t)(pos - start);
        char* part = (char*)malloc(len + 1);
        memcpy(part, start, len);
        part[len] = '\0';
        cool_list_push(out, cv_str(part));
        start = pos + sep_len;
    }
    cool_list_push(out, cv_str(strdup(start)));
    return out;
}

static CoolVal cool_string_replace(CoolVal obj, CoolVal old_v, CoolVal new_v) {
    const char* s = (const char*)(intptr_t)obj.payload;
    const char* old = (const char*)(intptr_t)old_v.payload;
    const char* repl = (const char*)(intptr_t)new_v.payload;
    size_t old_len = strlen(old), repl_len = strlen(repl);
    if (old_len == 0) return cv_str(strdup(s));
    size_t count = 0;
    const char* p = s;
    while ((p = strstr(p, old)) != NULL) {
        count++;
        p += old_len;
    }
    size_t total = strlen(s) + count * (repl_len - old_len) + 1;
    char* out = (char*)malloc(total);
    char* dst = out;
    const char* cur = s;
    while ((p = strstr(cur, old)) != NULL) {
        size_t chunk = (size_t)(p - cur);
        memcpy(dst, cur, chunk);
        dst += chunk;
        memcpy(dst, repl, repl_len);
        dst += repl_len;
        cur = p + old_len;
    }
    strcpy(dst, cur);
    return cv_str(out);
}

static CoolVal cool_string_startswith(CoolVal obj, CoolVal prefix_v) {
    const char* s = (const char*)(intptr_t)obj.payload;
    const char* prefix = (const char*)(intptr_t)prefix_v.payload;
    size_t n = strlen(prefix);
    return cv_bool(strncmp(s, prefix, n) == 0);
}

static CoolVal cool_string_endswith(CoolVal obj, CoolVal suffix_v) {
    const char* s = (const char*)(intptr_t)obj.payload;
    const char* suffix = (const char*)(intptr_t)suffix_v.payload;
    size_t ls = strlen(s), lx = strlen(suffix);
    return cv_bool(ls >= lx && strcmp(s + ls - lx, suffix) == 0);
}

static CoolVal cool_string_find(CoolVal obj, CoolVal sub_v) {
    const char* s = (const char*)(intptr_t)obj.payload;
    const char* sub = (const char*)(intptr_t)sub_v.payload;
    const char* pos = strstr(s, sub);
    return cv_int(pos ? (int64_t)(pos - s) : -1);
}

static CoolVal cool_string_count(CoolVal obj, CoolVal sub_v) {
    const char* s = (const char*)(intptr_t)obj.payload;
    const char* sub = (const char*)(intptr_t)sub_v.payload;
    size_t sub_len = strlen(sub);
    if (sub_len == 0) return cv_int((int64_t)strlen(s) + 1);
    int64_t count = 0;
    const char* p = s;
    while ((p = strstr(p, sub)) != NULL) {
        count++;
        p += sub_len;
    }
    return cv_int(count);
}

static CoolVal cool_string_title(CoolVal obj) {
    const char* s = (const char*)(intptr_t)obj.payload;
    size_t len = strlen(s);
    char* out = (char*)malloc(len + 1);
    int new_word = 1;
    for (size_t i = 0; i < len; i++) {
        unsigned char ch = (unsigned char)s[i];
        out[i] = (char)(new_word ? toupper(ch) : tolower(ch));
        new_word = isspace(ch) ? 1 : 0;
    }
    out[len] = '\0';
    return cv_str(out);
}

static CoolVal cool_string_capitalize(CoolVal obj) {
    const char* s = (const char*)(intptr_t)obj.payload;
    size_t len = strlen(s);
    char* out = (char*)malloc(len + 1);
    if (len == 0) {
        out[0] = '\0';
        return cv_str(out);
    }
    out[0] = (char)toupper((unsigned char)s[0]);
    for (size_t i = 1; i < len; i++) out[i] = (char)tolower((unsigned char)s[i]);
    out[len] = '\0';
    return cv_str(out);
}

static CoolVal cool_string_format(CoolVal obj, int32_t nargs, CoolVal* args) {
    const char* s = (const char*)(intptr_t)obj.payload;
    CoolStrBuf sb;
    sb_init(&sb);
    int32_t argi = 0;
    while (*s) {
        if (s[0] == '{' && s[1] == '}' && argi < nargs) {
            char* part = cool_to_str(args[argi++]);
            sb_push_str(&sb, part);
            s += 2;
        } else {
            sb_push_char(&sb, *s++);
        }
    }
    return cv_str(sb.data);
}

static CoolVal cool_list_reverse_copy(CoolVal seq) {
    if (seq.tag != TAG_LIST) {
        fprintf(stderr, "TypeError: list.reverse() requires a list\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)seq.payload;
    CoolVal out = cool_list_make(cv_int(src->length));
    for (int64_t i = src->length - 1; i >= 0; i--) {
        cool_list_push(out, ((CoolVal*)src->data)[i]);
    }
    return out;
}

static CoolVal cool_list_flatten_copy(CoolVal seq) {
    if (seq.tag != TAG_LIST) {
        fprintf(stderr, "TypeError: list.flatten() requires a list\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)seq.payload;
    CoolVal out = cool_list_make(cv_int(src->length));
    for (int64_t i = 0; i < src->length; i++) {
        CoolVal item = ((CoolVal*)src->data)[i];
        if (item.tag == TAG_LIST) {
            CoolList* inner = (CoolList*)(intptr_t)item.payload;
            for (int64_t j = 0; j < inner->length; j++) {
                cool_list_push(out, ((CoolVal*)inner->data)[j]);
            }
        } else {
            cool_list_push(out, item);
        }
    }
    return out;
}

static CoolVal cool_list_unique_copy(CoolVal seq) {
    if (seq.tag != TAG_LIST) {
        fprintf(stderr, "TypeError: list.unique() requires a list\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)seq.payload;
    CoolVal out = cool_list_make(cv_int(src->length));
    for (int64_t i = 0; i < src->length; i++) {
        CoolVal item = ((CoolVal*)src->data)[i];
        if (!cool_truthy(cool_list_contains_local(out, item))) {
            cool_list_push(out, item);
        }
    }
    return out;
}

static CoolVal cool_call_callable1(CoolVal callable, CoolVal arg) {
    if (!cool_is_closure(callable)) {
        fprintf(stderr, "TypeError: callable argument must be a function\n");
        exit(1);
    }
    int64_t fn_ptr = cool_closure_get_fn_ptr(callable);
    CoolVal argv[1] = {arg};
    return call_cool_fn_ptr(fn_ptr, 1, argv);
}

static CoolVal cool_call_callable2(CoolVal callable, CoolVal arg1, CoolVal arg2) {
    if (!cool_is_closure(callable)) {
        fprintf(stderr, "TypeError: callable argument must be a function\n");
        exit(1);
    }
    int64_t fn_ptr = cool_closure_get_fn_ptr(callable);
    CoolVal argv[2] = {arg1, arg2};
    return call_cool_fn_ptr(fn_ptr, 2, argv);
}

static void cool_test_raisef(const char* fmt, ...) {
    char buf[512];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    cool_raise(cv_str(strdup(buf)));
}

static CoolVal cool_test_raise_assertion(const char* message) {
    CoolStrBuf sb;
    sb_init(&sb);
    sb_push_str(&sb, "AssertionError: ");
    sb_push_str(&sb, message ? message : "assertion failed");
    cool_raise(cv_str(sb.data));
    return cv_nil();
}

static int cool_test_copy_args(CoolVal value, CoolVal* out, int32_t* out_count) {
    if (value.tag == TAG_NIL) {
        *out_count = 0;
        return 1;
    }
    if (value.tag != TAG_LIST && value.tag != TAG_TUPLE) {
        cool_test_raisef("test.raises() args must be a list or tuple, got %s", cool_type_name(value.tag));
    }
    CoolList* items = (CoolList*)(intptr_t)value.payload;
    if (items->length > 8) {
        cool_test_raisef("test.raises() supports at most 8 arguments in native mode");
    }
    for (int64_t i = 0; i < items->length; i++) {
        out[i] = ((CoolVal*)items->data)[i];
    }
    *out_count = (int32_t)items->length;
    return 1;
}

static CoolVal cool_test_call_callable(CoolVal callable, int32_t argc, CoolVal* argv) {
    if (cool_is_closure(callable)) {
        int64_t fn_ptr = cool_closure_get_fn_ptr(callable);
        return call_cool_fn_ptr(fn_ptr, argc, argv);
    }
    if (callable.tag == TAG_FFI_FUNC) {
        switch (argc) {
            case 0: return cool_ffi_call(callable, 0);
            case 1: return cool_ffi_call(callable, 1, argv[0]);
            case 2: return cool_ffi_call(callable, 2, argv[0], argv[1]);
            case 3: return cool_ffi_call(callable, 3, argv[0], argv[1], argv[2]);
            case 4: return cool_ffi_call(callable, 4, argv[0], argv[1], argv[2], argv[3]);
            case 5: return cool_ffi_call(callable, 5, argv[0], argv[1], argv[2], argv[3], argv[4]);
            case 6: return cool_ffi_call(callable, 6, argv[0], argv[1], argv[2], argv[3], argv[4], argv[5]);
            case 7: return cool_ffi_call(callable, 7, argv[0], argv[1], argv[2], argv[3], argv[4], argv[5], argv[6]);
            case 8:
                return cool_ffi_call(callable, 8, argv[0], argv[1], argv[2], argv[3], argv[4], argv[5], argv[6], argv[7]);
            default:
                fprintf(stderr, "RuntimeError: too many arguments for ffi function call (%d)\n", argc);
                exit(1);
        }
    }
    return cool_noncallable(callable);
}

static int cool_test_matches_expected(CoolVal exc, CoolVal expected) {
    if (expected.tag == TAG_NIL) return 1;
    if (expected.tag == TAG_CLASS) {
        CoolClass* cls = (CoolClass*)(intptr_t)expected.payload;
        if (!cls) return 0;
        return cool_exception_matches(exc, (const char*)(intptr_t)cls->name);
    }
    if (expected.tag != TAG_STR) {
        cool_test_raisef(
            "test.raises() expected exception must be a string/class or nil, got %s",
            cool_type_name(expected.tag)
        );
    }
    const char* expected_name = (const char*)(intptr_t)expected.payload;
    if (exc.tag == TAG_STR) {
        const char* text = (const char*)(intptr_t)exc.payload;
        size_t n = strlen(expected_name);
        return strcmp(text, expected_name) == 0
            || (strncmp(text, expected_name, n) == 0 && text[n] == ':')
            || strcmp(expected_name, "Exception") == 0;
    }
    if (exc.tag == TAG_OBJECT) {
        return cool_exception_matches(exc, expected_name);
    }
    return strcmp(cool_type_name(exc.tag), expected_name) == 0 || strcmp(expected_name, "Exception") == 0;
}

static CoolVal cool_test_raises(CoolVal callable, CoolVal args_value, CoolVal expected) {
    CoolVal call_args[8];
    int32_t argc = 0;
    cool_test_copy_args(args_value, call_args, &argc);

    jmp_buf buf;
    cool_enter_try(&buf);
    if (setjmp(buf) == 0) {
        cool_test_call_callable(callable, argc, argc > 0 ? call_args : NULL);
        cool_exit_try();
        return cool_test_raise_assertion("expected exception, but call returned successfully");
    }

    cool_exit_try();
    CoolVal exc = cool_get_exception();
    if (!cool_test_matches_expected(exc, expected)) {
        cool_test_raisef(
            "expected exception %s, got %s",
            cool_to_str(expected),
            cool_to_str(exc)
        );
    }
    return exc;
}

static void cool_csv_raisef(const char* fmt, ...) {
    char buf[512];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    cool_raise(cv_str(strdup(buf)));
}

static void cool_csv_reset_field(CoolStrBuf* field) {
    field->len = 0;
    field->data[0] = '\0';
}

static void cool_csv_push_field(CoolVal row, CoolStrBuf* field) {
    cool_list_push(row, cv_str(strdup(field->data)));
    cool_csv_reset_field(field);
}

static const char* cool_csv_value_str(CoolVal value) {
    return value.tag == TAG_STR ? (const char*)(intptr_t)value.payload : cool_to_str(value);
}

static CoolVal cool_csv_rows(CoolVal text) {
    if (text.tag != TAG_STR) {
        cool_csv_raisef("csv.rows() requires a string, got %s", cool_type_name(text.tag));
    }

    const char* input = (const char*)(intptr_t)text.payload;
    CoolVal rows = cool_list_make(cv_int(8));
    CoolVal row = cool_list_make(cv_int(4));
    CoolStrBuf field;
    sb_init(&field);
    int in_quotes = 0;
    int just_closed_quote = 0;
    int just_finished_row = 0;
    int saw_any = 0;

    for (const char* p = input; *p; ) {
        char c = *p++;
        saw_any = 1;
        if (in_quotes) {
            if (c == '"') {
                if (*p == '"') {
                    sb_push_char(&field, '"');
                    p++;
                } else {
                    in_quotes = 0;
                    just_closed_quote = 1;
                }
            } else {
                sb_push_char(&field, c);
            }
            continue;
        }

        switch (c) {
            case '"':
                if (field.len != 0 || just_closed_quote) {
                    free(field.data);
                    cool_csv_raisef("csv parse error: quote must start at beginning of field");
                }
                in_quotes = 1;
                just_finished_row = 0;
                break;
            case ',':
                cool_csv_push_field(row, &field);
                just_closed_quote = 0;
                just_finished_row = 0;
                break;
            case '\r':
                if (*p == '\n') p++;
                cool_csv_push_field(row, &field);
                cool_list_push(rows, row);
                row = cool_list_make(cv_int(4));
                just_closed_quote = 0;
                just_finished_row = 1;
                break;
            case '\n':
                cool_csv_push_field(row, &field);
                cool_list_push(rows, row);
                row = cool_list_make(cv_int(4));
                just_closed_quote = 0;
                just_finished_row = 1;
                break;
            default:
                if (just_closed_quote) {
                    free(field.data);
                    cool_csv_raisef("csv parse error: unexpected character after closing quote");
                }
                sb_push_char(&field, c);
                just_finished_row = 0;
                break;
        }
    }

    if (in_quotes) {
        free(field.data);
        cool_csv_raisef("csv parse error: unterminated quoted field");
    }

    if (saw_any && (!just_finished_row || field.len > 0 || cool_list_len(row).payload > 0)) {
        cool_csv_push_field(row, &field);
        cool_list_push(rows, row);
    }
    free(field.data);
    return rows;
}

static CoolVal cool_csv_dicts(CoolVal text) {
    CoolVal parsed_rows = cool_csv_rows(text);
    CoolList* rows = (CoolList*)(intptr_t)parsed_rows.payload;
    CoolVal out = cool_list_make(cv_int(rows->length > 0 ? rows->length - 1 : 0));
    if (rows->length == 0) return out;

    CoolVal header_row = ((CoolVal*)rows->data)[0];
    CoolList* headers = (CoolList*)(intptr_t)header_row.payload;
    for (int64_t row_idx = 1; row_idx < rows->length; row_idx++) {
        CoolVal row_val = ((CoolVal*)rows->data)[row_idx];
        CoolList* row = (CoolList*)(intptr_t)row_val.payload;
        CoolVal dict = cool_dict_new();
        for (int64_t col_idx = 0; col_idx < headers->length; col_idx++) {
            CoolVal key = ((CoolVal*)headers->data)[col_idx];
            CoolVal value = col_idx < row->length ? ((CoolVal*)row->data)[col_idx] : cv_str("");
            cool_dict_set(dict, key, value);
        }
        cool_list_push(out, dict);
    }
    return out;
}

static void cool_csv_write_field(CoolStrBuf* out, const char* field) {
    size_t len = strlen(field);
    int needs_quotes = 0;
    if (len > 0 && (isspace((unsigned char)field[0]) || isspace((unsigned char)field[len - 1]))) {
        needs_quotes = 1;
    }
    for (size_t i = 0; i < len && !needs_quotes; i++) {
        char c = field[i];
        if (c == ',' || c == '"' || c == '\n' || c == '\r') {
            needs_quotes = 1;
        }
    }
    if (!needs_quotes) {
        sb_push_str(out, field);
        return;
    }
    sb_push_char(out, '"');
    for (size_t i = 0; i < len; i++) {
        if (field[i] == '"') {
            sb_push_char(out, '"');
            sb_push_char(out, '"');
        } else {
            sb_push_char(out, field[i]);
        }
    }
    sb_push_char(out, '"');
}

static void cool_csv_write_sequence_row(CoolStrBuf* out, CoolVal row_val) {
    if (row_val.tag != TAG_LIST && row_val.tag != TAG_TUPLE) {
        cool_csv_raisef(
            "csv.write() rows must contain only lists, tuples, or dicts, got %s",
            cool_type_name(row_val.tag)
        );
    }
    CoolList* row = (CoolList*)(intptr_t)row_val.payload;
    for (int64_t col_idx = 0; col_idx < row->length; col_idx++) {
        if (col_idx > 0) sb_push_char(out, ',');
        cool_csv_write_field(out, cool_csv_value_str(((CoolVal*)row->data)[col_idx]));
    }
}

static CoolVal cool_csv_write(CoolVal rows_val) {
    if (rows_val.tag != TAG_LIST && rows_val.tag != TAG_TUPLE) {
        cool_csv_raisef("csv.write() rows must be a list or tuple, got %s", cool_type_name(rows_val.tag));
    }

    CoolList* rows = (CoolList*)(intptr_t)rows_val.payload;
    if (rows->length == 0) return cv_str("");

    CoolVal first = ((CoolVal*)rows->data)[0];
    CoolStrBuf out;
    sb_init(&out);

    if (first.tag == TAG_DICT) {
        CoolDict* first_dict = (CoolDict*)(intptr_t)first.payload;
        for (int64_t col_idx = 0; col_idx < first_dict->len; col_idx++) {
            if (col_idx > 0) sb_push_char(&out, ',');
            cool_csv_write_field(&out, cool_csv_value_str(first_dict->keys[col_idx]));
        }
        for (int64_t row_idx = 0; row_idx < rows->length; row_idx++) {
            CoolVal row_val = ((CoolVal*)rows->data)[row_idx];
            if (row_val.tag != TAG_DICT) {
                cool_csv_raisef(
                    "csv.write() rows must all be dicts when the first row is a dict, got %s",
                    cool_type_name(row_val.tag)
                );
            }
            sb_push_char(&out, '\n');
            for (int64_t col_idx = 0; col_idx < first_dict->len; col_idx++) {
                if (col_idx > 0) sb_push_char(&out, ',');
                CoolVal key = first_dict->keys[col_idx];
                const char* field = "";
                if (cool_truthy(cool_dict_contains(row_val, key))) {
                    field = cool_csv_value_str(cool_dict_get_opt(row_val, key));
                }
                cool_csv_write_field(&out, field);
            }
        }
        return cv_str(out.data);
    }

    for (int64_t row_idx = 0; row_idx < rows->length; row_idx++) {
        if (row_idx > 0) sb_push_char(&out, '\n');
        cool_csv_write_sequence_row(&out, ((CoolVal*)rows->data)[row_idx]);
    }
    return cv_str(out.data);
}

static CoolVal cool_list_map_copy(CoolVal func, CoolVal seq) {
    if (seq.tag != TAG_LIST) {
        fprintf(stderr, "TypeError: list.map() requires a list\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)seq.payload;
    CoolVal out = cool_list_make(cv_int(src->length));
    for (int64_t i = 0; i < src->length; i++) {
        CoolVal item = ((CoolVal*)src->data)[i];
        cool_list_push(out, cool_call_callable1(func, item));
    }
    return out;
}

static CoolVal cool_list_filter_copy(CoolVal func, CoolVal seq) {
    if (seq.tag != TAG_LIST) {
        fprintf(stderr, "TypeError: list.filter() requires a list\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)seq.payload;
    CoolVal out = cool_list_make(cv_int(src->length));
    for (int64_t i = 0; i < src->length; i++) {
        CoolVal item = ((CoolVal*)src->data)[i];
        if (cool_truthy(cool_call_callable1(func, item))) {
            cool_list_push(out, item);
        }
    }
    return out;
}

static CoolVal cool_list_reduce_copy(CoolVal func, CoolVal seq, CoolVal initial, int has_initial) {
    if (seq.tag != TAG_LIST) {
        fprintf(stderr, "TypeError: list.reduce() requires a list\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)seq.payload;
    if (src->length == 0 && !has_initial) {
        fprintf(stderr, "ValueError: list.reduce() called on empty list with no initial value\n");
        exit(1);
    }
    int64_t idx = 0;
    CoolVal acc = initial;
    if (!has_initial) {
        acc = ((CoolVal*)src->data)[0];
        idx = 1;
    }
    for (; idx < src->length; idx++) {
        acc = cool_call_callable2(func, acc, ((CoolVal*)src->data)[idx]);
    }
    return acc;
}

static CoolVal g_queue_class = { TAG_NIL, 0 };
static CoolVal g_stack_class = { TAG_NIL, 0 };

static CoolVal collections_get_items(CoolVal self) {
    CoolVal items = cool_get_attr(self, "items");
    if (items.tag != TAG_LIST) {
        items = cool_list_make(cv_int(4));
        cool_set_attr(self, "items", items);
    }
    return items;
}

static CoolVal collections_queue_push(CoolVal self, CoolVal item) {
    cool_list_push(collections_get_items(self), item);
    return cv_nil();
}

static CoolVal collections_queue_enqueue(CoolVal self, CoolVal item) {
    return collections_queue_push(self, item);
}

static CoolVal collections_queue_pop(CoolVal self) {
    CoolVal items = collections_get_items(self);
    CoolList* lst = (CoolList*)(intptr_t)items.payload;
    if (lst->length == 0) cool_raise(cv_str("Queue is empty"));
    CoolVal first = ((CoolVal*)lst->data)[0];
    for (int64_t i = 1; i < lst->length; i++) {
        ((CoolVal*)lst->data)[i - 1] = ((CoolVal*)lst->data)[i];
    }
    lst->length--;
    return first;
}

static CoolVal collections_queue_dequeue(CoolVal self) {
    return collections_queue_pop(self);
}

static CoolVal collections_queue_peek(CoolVal self) {
    CoolVal items = collections_get_items(self);
    CoolList* lst = (CoolList*)(intptr_t)items.payload;
    if (lst->length == 0) cool_raise(cv_str("Queue is empty"));
    return ((CoolVal*)lst->data)[0];
}

static CoolVal collections_queue_is_empty(CoolVal self) {
    CoolVal items = collections_get_items(self);
    return cv_bool(cool_len(items).payload == 0);
}

static CoolVal collections_queue_size(CoolVal self) {
    return cool_len(collections_get_items(self));
}

static CoolVal collections_stack_push(CoolVal self, CoolVal item) {
    cool_list_push(collections_get_items(self), item);
    return cv_nil();
}

static CoolVal collections_stack_pop(CoolVal self) {
    CoolVal items = collections_get_items(self);
    CoolList* lst = (CoolList*)(intptr_t)items.payload;
    if (lst->length == 0) cool_raise(cv_str("Stack is empty"));
    return cool_list_pop(items);
}

static CoolVal collections_stack_peek(CoolVal self) {
    CoolVal items = collections_get_items(self);
    CoolList* lst = (CoolList*)(intptr_t)items.payload;
    if (lst->length == 0) cool_raise(cv_str("Stack is empty"));
    return ((CoolVal*)lst->data)[lst->length - 1];
}

static CoolVal collections_stack_is_empty(CoolVal self) {
    CoolVal items = collections_get_items(self);
    return cv_bool(cool_len(items).payload == 0);
}

static CoolVal collections_stack_size(CoolVal self) {
    return cool_len(collections_get_items(self));
}

static void cool_init_collections_classes(void) {
    if (g_queue_class.tag == TAG_CLASS && g_stack_class.tag == TAG_CLASS) return;

    int64_t queue_methods[] = {
        (int64_t)(intptr_t)"method_push", (int64_t)(intptr_t)collections_queue_push,
        (int64_t)(intptr_t)"method_enqueue", (int64_t)(intptr_t)collections_queue_enqueue,
        (int64_t)(intptr_t)"method_pop", (int64_t)(intptr_t)collections_queue_pop,
        (int64_t)(intptr_t)"method_dequeue", (int64_t)(intptr_t)collections_queue_dequeue,
        (int64_t)(intptr_t)"method_peek", (int64_t)(intptr_t)collections_queue_peek,
        (int64_t)(intptr_t)"method_is_empty", (int64_t)(intptr_t)collections_queue_is_empty,
        (int64_t)(intptr_t)"method_size", (int64_t)(intptr_t)collections_queue_size,
    };
    g_queue_class = cool_class_new("Queue", cv_nil(), 7, queue_methods);

    int64_t stack_methods[] = {
        (int64_t)(intptr_t)"method_push", (int64_t)(intptr_t)collections_stack_push,
        (int64_t)(intptr_t)"method_pop", (int64_t)(intptr_t)collections_stack_pop,
        (int64_t)(intptr_t)"method_peek", (int64_t)(intptr_t)collections_stack_peek,
        (int64_t)(intptr_t)"method_is_empty", (int64_t)(intptr_t)collections_stack_is_empty,
        (int64_t)(intptr_t)"method_size", (int64_t)(intptr_t)collections_stack_size,
    };
    g_stack_class = cool_class_new("Stack", cv_nil(), 5, stack_methods);
}

static CoolVal collections_make_instance(CoolVal cls) {
    CoolVal obj = cool_object_new(cls);
    cool_set_attr(obj, "items", cool_list_make(cv_int(4)));
    return obj;
}

static CoolFile* cv_file_ptr(CoolVal v) {
    return (CoolFile*)(intptr_t)v.payload;
}

CoolVal cool_file_open(CoolVal path, CoolVal mode) {
    if (path.tag != TAG_STR) {
        fprintf(stderr, "TypeError: open() requires a path string\n");
        exit(1);
    }
    if (mode.tag != TAG_STR) {
        fprintf(stderr, "TypeError: open() mode must be a string\n");
        exit(1);
    }
    const char* p = (const char*)(intptr_t)path.payload;
    const char* m = (const char*)(intptr_t)mode.payload;
    FILE* fp = fopen(p, m);
    if (!fp) {
        fprintf(stderr, "FileNotFoundError: '%s'\n", p);
        exit(1);
    }
    CoolFile* f = (CoolFile*)malloc(sizeof(CoolFile));
    if (!f) {
        fprintf(stderr, "RuntimeError: out of memory opening file\n");
        exit(1);
    }
    f->fp = fp;
    f->closed = 0;
    CoolVal v;
    v.tag = TAG_FILE;
    v.payload = (int64_t)(intptr_t)f;
    return v;
}

CoolVal cool_file_read(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (!f || f->closed) {
        fputs("ValueError: I/O operation on closed file\n", stderr);
        exit(1);
    }
    fseek(f->fp, 0, SEEK_END);
    long size = ftell(f->fp);
    rewind(f->fp);
    char* buf = (char*)malloc((size_t)size + 1);
    if (!buf) return cv_str("");
    size_t read = fread(buf, 1, (size_t)size, f->fp);
    buf[read] = '\0';
    return cv_str(buf);
}

CoolVal cool_file_readline(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (!f || f->closed) {
        fputs("ValueError: I/O operation on closed file\n", stderr);
        exit(1);
    }
    char* buf = (char*)malloc(4096);
    size_t cap = 4096, len = 0;
    int c;
    while ((c = fgetc(f->fp)) != EOF) {
        if (len + 2 >= cap) {
            cap *= 2;
            buf = (char*)realloc(buf, cap);
        }
        buf[len++] = (char)c;
        if (c == '\n') break;
    }
    buf[len] = '\0';
    return cv_str(buf);
}

CoolVal cool_file_readlines(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (!f || f->closed) {
        fputs("ValueError: I/O operation on closed file\n", stderr);
        exit(1);
    }
    CoolVal res = cool_list_make(cv_int(4));
    char* buf = (char*)malloc(4096);
    size_t cap = 4096, len = 0;
    int c;
    while ((c = fgetc(f->fp)) != EOF) {
        if (len + 2 >= cap) {
            cap *= 2;
            buf = (char*)realloc(buf, cap);
        }
        buf[len++] = (char)c;
        if (c == '\n') {
            buf[len] = '\0';
            char* line = (char*)malloc(len + 1);
            memcpy(line, buf, len + 1);
            cool_list_push(res, cv_str(line));
            len = 0;
        }
    }
    if (len > 0) {
        buf[len] = '\0';
        char* line = (char*)malloc(len + 1);
        memcpy(line, buf, len + 1);
        cool_list_push(res, cv_str(line));
    }
    free(buf);
    return res;
}

CoolVal cool_file_write(CoolVal file, CoolVal text) {
    CoolFile* f = cv_file_ptr(file);
    if (!f || f->closed) {
        fputs("ValueError: I/O operation on closed file\n", stderr);
        exit(1);
    }
    const char* s = text.tag == TAG_STR ? (const char*)(intptr_t)text.payload : cool_to_str(text);
    fputs(s, f->fp);
    fflush(f->fp);
    return cv_nil();
}

CoolVal cool_file_writelines(CoolVal file, CoolVal lines) {
    if (lines.tag != TAG_LIST && lines.tag != TAG_TUPLE) {
        fprintf(stderr, "TypeError: writelines() requires a list or tuple\n");
        exit(1);
    }
    CoolList* l = (CoolList*)(intptr_t)lines.payload;
    for (int64_t i = 0; i < l->length; i++) {
        cool_file_write(file, ((CoolVal*)l->data)[i]);
    }
    return cv_nil();
}

CoolVal cool_file_close(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (f && !f->closed) {
        fclose(f->fp);
        f->closed = 1;
    }
    return cv_nil();
}

CoolVal cool_call_method_vararg(CoolVal obj, const char* name, int32_t nargs, ...) {
    va_list ap;
    va_start(ap, nargs);
    g_method_args[0] = obj;
    for (int32_t i = 0; i < nargs && i < 31; i++) {
        g_method_args[i + 1] = va_arg(ap, CoolVal);
    }
    g_method_arg_count = nargs + 1;
    va_end(ap);

    const char* builtin_name = strncmp(name, "method_", 7) == 0 ? name + 7 : name;

    if (obj.tag == TAG_STR) {
        if (strcmp(builtin_name, "upper") == 0 && nargs == 0) return cool_string_upper(obj);
        if (strcmp(builtin_name, "lower") == 0 && nargs == 0) return cool_string_lower(obj);
        if (strcmp(builtin_name, "strip") == 0 && nargs == 0) return cool_string_strip(obj);
        if (strcmp(builtin_name, "lstrip") == 0 && nargs == 0) return cool_string_lstrip(obj);
        if (strcmp(builtin_name, "rstrip") == 0 && nargs == 0) return cool_string_rstrip(obj);
        if (strcmp(builtin_name, "join") == 0 && nargs == 1) return cool_string_join(obj, g_method_args[1]);
    }

    if (obj.tag == TAG_DICT) {
        CoolVal callable = cool_dict_get_opt(obj, cv_str(builtin_name));
        if (callable.tag == TAG_CLOSURE) {
            int64_t fn_ptr = cool_closure_get_fn_ptr(callable);
            return call_cool_fn_ptr(fn_ptr, nargs, nargs > 0 ? &g_method_args[1] : NULL);
        }
        if (callable.tag == TAG_FFI_FUNC) {
            switch (nargs) {
                case 0: return cool_ffi_call(callable, 0);
                case 1: return cool_ffi_call(callable, 1, g_method_args[1]);
                case 2: return cool_ffi_call(callable, 2, g_method_args[1], g_method_args[2]);
                case 3: return cool_ffi_call(callable, 3, g_method_args[1], g_method_args[2], g_method_args[3]);
                case 4: return cool_ffi_call(callable, 4, g_method_args[1], g_method_args[2], g_method_args[3], g_method_args[4]);
                default:
                    fprintf(stderr, "RuntimeError: too many arguments for ffi method call (%d)\n", nargs);
                    exit(1);
            }
        }
    }

    if (obj.tag == TAG_LIST && strcmp(builtin_name, "append") == 0 && nargs == 1) {
        cool_list_push(obj, g_method_args[1]);
        return cv_nil();
    }
    if (obj.tag == TAG_DICT && (strcmp(builtin_name, "contains") == 0 || strcmp(builtin_name, "has_key") == 0) && nargs == 1) {
        return cool_dict_contains(obj, g_method_args[1]);
    }
    if (obj.tag == TAG_FILE) {
        CoolVal a0 = nargs > 0 ? g_method_args[1] : cv_nil();
        if (strcmp(builtin_name, "__enter__") == 0 && nargs == 0) return obj;
        if (strcmp(builtin_name, "__exit__") == 0 && nargs == 3) return cool_file_close(obj);
        if (strcmp(builtin_name, "read") == 0 && nargs == 0) return cool_file_read(obj);
        if (strcmp(builtin_name, "readline") == 0 && nargs == 0) return cool_file_readline(obj);
        if (strcmp(builtin_name, "readlines") == 0 && nargs == 0) return cool_file_readlines(obj);
        if (strcmp(builtin_name, "write") == 0 && nargs == 1) return cool_file_write(obj, a0);
        if (strcmp(builtin_name, "writelines") == 0 && nargs == 1) return cool_file_writelines(obj, a0);
        if (strcmp(builtin_name, "close") == 0 && nargs == 0) return cool_file_close(obj);
    }

    if (obj.tag != TAG_OBJECT) return cv_nil();
    CoolObject* o = (CoolObject*)(intptr_t)obj.payload;
    if (!o->class) return cv_nil();

    int64_t method_ptr = cool_get_method_ptr((CoolVal){TAG_CLASS, (int64_t)(intptr_t)o->class}, name);
    if (method_ptr == 0) {
        fprintf(stderr, "AttributeError: '%s' object has no attribute '%s'\n",
                (const char*)(intptr_t)o->class->name, name);
        exit(1);
    }
    return call_cool_fn_ptr(method_ptr, nargs + 1, g_method_args);
}

CoolVal cool_get_arg(int32_t idx) {
    if (idx < 0 || idx >= g_method_arg_count) return cv_nil();
    return g_method_args[idx];
}

/* Set a global argument for constructor/method calls */
void cool_set_global_arg(int32_t idx, CoolVal val) {
    if (idx < 0 || idx >= 32) return;
    g_method_args[idx] = val;
    if (idx >= g_method_arg_count) g_method_arg_count = idx + 1;
}

CoolVal cool_is_instance(CoolVal obj, const char* class_name) {
    if (obj.tag != TAG_OBJECT) return cv_bool(0);
    CoolObject* o = (CoolObject*)(intptr_t)obj.payload;
    if (!o->class) return cv_bool(0);
    const char* actual_name = (const char*)(intptr_t)o->class->name;
    return cv_bool(cool_class_name_matches(actual_name, class_name));
}

int32_t cool_exception_matches(CoolVal exc, const char* class_name) {
    if (exc.tag == TAG_STR) {
        const char* text = (const char*)(intptr_t)exc.payload;
        return strcmp(text, class_name) == 0
            || strcmp(class_name, "Exception") == 0
            || strcmp(class_name, "Error") == 0;
    }
    if (exc.tag == TAG_OBJECT) {
        return cool_truthy(cool_is_instance(exc, class_name));
    }
    return strcmp(class_name, "Exception") == 0;
}

/* ── Dict runtime ────────────────────────────────────────────────────── */

CoolVal cool_dict_new(void) {
    CoolDict* d = (CoolDict*)malloc(sizeof(CoolDict));
    d->tag = TAG_DICT;
    d->len = 0;
    d->cap = 8;
    d->keys = (CoolVal*)malloc(8 * sizeof(CoolVal));
    d->vals = (CoolVal*)malloc(8 * sizeof(CoolVal));
    CoolVal v;
    v.tag = TAG_DICT;
    v.payload = (int64_t)(intptr_t)d;
    return v;
}

CoolVal cool_dict_set(CoolVal dict_v, CoolVal key, CoolVal val) {
    if (dict_v.tag != TAG_DICT) { fprintf(stderr, "TypeError: not a dict\n"); exit(1); }
    CoolDict* d = (CoolDict*)(intptr_t)dict_v.payload;
    for (int64_t i = 0; i < d->len; i++) {
        if (cv_eq_raw(d->keys[i], key)) { d->vals[i] = val; return dict_v; }
    }
    if (d->len == d->cap) {
        d->cap *= 2;
        d->keys = (CoolVal*)realloc(d->keys, d->cap * sizeof(CoolVal));
        d->vals = (CoolVal*)realloc(d->vals, d->cap * sizeof(CoolVal));
    }
    d->keys[d->len] = key;
    d->vals[d->len] = val;
    d->len++;
    return dict_v;
}

CoolVal cool_dict_get(CoolVal dict_v, CoolVal key) {
    if (dict_v.tag != TAG_DICT) { fprintf(stderr, "TypeError: not a dict\n"); exit(1); }
    CoolDict* d = (CoolDict*)(intptr_t)dict_v.payload;
    for (int64_t i = 0; i < d->len; i++) {
        if (cv_eq_raw(d->keys[i], key)) return d->vals[i];
    }
    fprintf(stderr, "KeyError\n"); exit(1);
}

CoolVal cool_dict_get_opt(CoolVal dict_v, CoolVal key) {
    if (dict_v.tag != TAG_DICT) return cv_nil();
    CoolDict* d = (CoolDict*)(intptr_t)dict_v.payload;
    for (int64_t i = 0; i < d->len; i++) {
        if (cv_eq_raw(d->keys[i], key)) return d->vals[i];
    }
    return cv_nil();
}

CoolVal cool_dict_len(CoolVal dict_v) {
    if (dict_v.tag != TAG_DICT) { fprintf(stderr, "TypeError: not a dict\n"); exit(1); }
    CoolDict* d = (CoolDict*)(intptr_t)dict_v.payload;
    CoolVal v; v.tag = TAG_INT; v.payload = d->len; return v;
}

CoolVal cool_dict_contains(CoolVal dict_v, CoolVal key) {
    if (dict_v.tag != TAG_DICT) return cv_bool(0);
    CoolDict* d = (CoolDict*)(intptr_t)dict_v.payload;
    for (int64_t i = 0; i < d->len; i++)
        if (cv_eq_raw(d->keys[i], key)) return cv_bool(1);
    return cv_bool(0);
}

/* Unified index: dispatches list, tuple, dict */
CoolVal cool_index(CoolVal obj, CoolVal idx) {
    if (obj.tag == TAG_LIST || obj.tag == TAG_TUPLE) return cool_list_get(obj, idx);
    if (obj.tag == TAG_DICT) return cool_dict_get(obj, idx);
    fprintf(stderr, "TypeError: not subscriptable\n"); exit(1);
}

CoolVal cool_slice(CoolVal obj, CoolVal start_v, CoolVal stop_v) {
    int64_t start = start_v.tag == TAG_NIL ? 0 : start_v.payload;
    int64_t stop = stop_v.tag == TAG_NIL ? INT64_MAX : stop_v.payload;
    if (obj.tag == TAG_LIST || obj.tag == TAG_TUPLE) {
        CoolList* src = (CoolList*)(intptr_t)obj.payload;
        int64_t len = src->length;
        if (start < 0) start += len;
        if (stop == INT64_MAX) stop = len;
        if (stop < 0) stop += len;
        if (start < 0) start = 0;
        if (stop > len) stop = len;
        if (start > stop) start = stop;
        CoolVal out = obj.tag == TAG_TUPLE ? cool_tuple_make(cv_int(stop - start)) : cool_list_make(cv_int(stop - start));
        for (int64_t i = start; i < stop; i++) {
            cool_list_push(out, ((CoolVal*)src->data)[i]);
        }
        return out;
    }
    if (obj.tag == TAG_STR) {
        const char* s = (const char*)(intptr_t)obj.payload;
        int64_t len = (int64_t)strlen(s);
        if (start < 0) start += len;
        if (stop == INT64_MAX) stop = len;
        if (stop < 0) stop += len;
        if (start < 0) start = 0;
        if (stop > len) stop = len;
        if (start > stop) start = stop;
        char* out = (char*)malloc((size_t)(stop - start + 1));
        memcpy(out, s + start, (size_t)(stop - start));
        out[stop - start] = '\0';
        return cv_str(out);
    }
    fprintf(stderr, "TypeError: not sliceable\n");
    exit(1);
}

/* Unified setindex: dispatches list vs dict */
CoolVal cool_setindex(CoolVal obj, CoolVal idx, CoolVal val) {
    if (obj.tag == TAG_LIST) return cool_list_set(obj, idx, val);
    if (obj.tag == TAG_DICT) return cool_dict_set(obj, idx, val);
    fprintf(stderr, "TypeError: not subscriptable\n"); exit(1);
}

CoolVal cool_round(CoolVal num, CoolVal digits) {
    double f = cv_to_float(num);
    if (digits.tag == TAG_NIL) return num.tag == TAG_INT ? num : cv_int((int64_t)llround(f));
    int64_t places = digits.payload;
    double factor = pow(10.0, (double)places);
    return cv_float(round(f * factor) / factor);
}

static int cool_compare(CoolVal a, CoolVal b) {
    if (cv_eq_raw(a, b)) return 0;
    return cool_truthy(cool_lt(a, b)) ? -1 : 1;
}

CoolVal cool_sorted(CoolVal iterable) {
    if (iterable.tag == TAG_STR) {
        const char* s = (const char*)(intptr_t)iterable.payload;
        CoolVal chars = cool_list_make(cv_int((int64_t)strlen(s)));
        for (const char* p = s; *p; p++) {
            char* ch = (char*)malloc(2);
            ch[0] = *p;
            ch[1] = '\0';
            cool_list_push(chars, cv_str(ch));
        }
        iterable = chars;
    }
    if (iterable.tag != TAG_LIST && iterable.tag != TAG_TUPLE) {
        fprintf(stderr, "TypeError: sorted() requires an iterable\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)iterable.payload;
    CoolVal out = cool_list_make(cv_int(src->length));
    for (int64_t i = 0; i < src->length; i++) cool_list_push(out, ((CoolVal*)src->data)[i]);
    CoolList* dst = (CoolList*)(intptr_t)out.payload;
    for (int64_t i = 0; i < dst->length; i++) {
        for (int64_t j = i + 1; j < dst->length; j++) {
            if (cool_compare(((CoolVal*)dst->data)[j], ((CoolVal*)dst->data)[i]) < 0) {
                CoolVal tmp = ((CoolVal*)dst->data)[i];
                ((CoolVal*)dst->data)[i] = ((CoolVal*)dst->data)[j];
                ((CoolVal*)dst->data)[j] = tmp;
            }
        }
    }
    return out;
}

CoolVal cool_sum(CoolVal iterable) {
    if (iterable.tag != TAG_LIST && iterable.tag != TAG_TUPLE) {
        fprintf(stderr, "TypeError: sum() requires a list or tuple\n");
        exit(1);
    }
    CoolList* src = (CoolList*)(intptr_t)iterable.payload;
    CoolVal total = cv_int(0);
    for (int64_t i = 0; i < src->length; i++) total = cool_add(total, ((CoolVal*)src->data)[i]);
    return total;
}

CoolVal cool_abs(CoolVal v) {
    if (v.tag == TAG_INT) return cv_int(llabs(v.payload));
    if (v.tag == TAG_FLOAT) return cv_float(fabs(cv_as_float(v)));
    fprintf(stderr, "TypeError: abs() requires a number\n");
    exit(1);
}

CoolVal cool_to_int(CoolVal v) {
    switch (v.tag) {
        case TAG_INT: return v;
        case TAG_FLOAT: return cv_int((int64_t)cv_as_float(v));
        case TAG_BOOL: return cv_int(v.payload ? 1 : 0);
        case TAG_STR: {
            const char* s = (const char*)(intptr_t)v.payload;
            while (*s && isspace((unsigned char)*s)) s++;
            char* end = NULL;
            long long n = strtoll(s, &end, 0);
            if (end == s) {
                fprintf(stderr, "ValueError: invalid int\n");
                exit(1);
            }
            return cv_int((int64_t)n);
        }
        default:
            fprintf(stderr, "TypeError: cannot convert to int\n");
            exit(1);
    }
}

CoolVal cool_to_float_val(CoolVal v) {
    switch (v.tag) {
        case TAG_FLOAT: return v;
        case TAG_INT: return cv_float((double)v.payload);
        case TAG_STR: {
            const char* s = (const char*)(intptr_t)v.payload;
            while (*s && isspace((unsigned char)*s)) s++;
            char* end = NULL;
            double n = strtod(s, &end);
            if (end == s) {
                fprintf(stderr, "ValueError: invalid float\n");
                exit(1);
            }
            return cv_float(n);
        }
        default:
            fprintf(stderr, "TypeError: cannot convert to float\n");
            exit(1);
    }
}

CoolVal cool_to_bool_val(CoolVal v) {
    return cv_bool(cool_truthy(v));
}

static CoolVal cool_make_argv(void) {
    CoolVal out = cool_list_make(cv_int(8));
    if (COOL_SCRIPT_PATH && *COOL_SCRIPT_PATH) {
        cool_list_push(out, cv_str(strdup(COOL_SCRIPT_PATH)));
    } else {
#ifdef __APPLE__
        char*** argvp = _NSGetArgv();
        int argc = *_NSGetArgc();
        if (argvp && *argvp) {
            for (int i = 0; i < argc; i++) {
                cool_list_push(out, cv_str(strdup((*argvp)[i])));
            }
        }
#elif defined(__linux__)
        FILE* f = fopen("/proc/self/cmdline", "rb");
        if (f) {
            char buf[4096];
            size_t n = fread(buf, 1, sizeof(buf), f);
            fclose(f);
            size_t start = 0;
            for (size_t i = 0; i < n; i++) {
                if (buf[i] == '\0') {
                    if (i > start) {
                        char* s = (char*)malloc(i - start + 1);
                        memcpy(s, &buf[start], i - start);
                        s[i - start] = '\0';
                        cool_list_push(out, cv_str(s));
                    }
                    start = i + 1;
                }
            }
        }
#endif
    }
    const char* extra = getenv("COOL_PROGRAM_ARGS");
    if (extra && *extra) {
        const char* start = extra;
        for (const char* p = extra;; p++) {
            if (*p == '\x1F' || *p == '\0') {
                size_t len = (size_t)(p - start);
                char* s = (char*)malloc(len + 1);
                memcpy(s, start, len);
                s[len] = '\0';
                cool_list_push(out, cv_str(s));
                if (*p == '\0') break;
                start = p + 1;
            }
        }
    }
    return out;
}

typedef enum {
    COOL_ARG_STR = 0,
    COOL_ARG_INT = 1,
    COOL_ARG_FLOAT = 2,
    COOL_ARG_BOOL = 3
} CoolArgType;

typedef struct {
    char* name;
    char* help;
    CoolArgType arg_type;
    int required;
    int has_default;
    CoolVal default_value;
} CoolArgPositional;

typedef struct {
    char* name;
    char* long_flag;
    char* short_flag;
    char* help;
    CoolArgType arg_type;
    int required;
    int has_default;
    CoolVal default_value;
} CoolArgOption;

typedef struct {
    char* prog;
    char* description;
    int positional_count;
    CoolArgPositional* positionals;
    int option_count;
    CoolArgOption* options;
} CoolArgParser;

static void argparse_failf(const char* fmt, ...) {
    char buf[1024];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    cool_raise(cv_str(strdup(buf)));
}

static int argparse_is_sequence(CoolVal value) {
    return value.tag == TAG_LIST || value.tag == TAG_TUPLE;
}

static int argparse_dict_has_key(CoolVal dict, const char* key) {
    if (dict.tag != TAG_DICT) return 0;
    return cool_truthy(cool_dict_contains(dict, cv_str(key)));
}

static CoolVal argparse_dict_get_opt(CoolVal dict, const char* key) {
    if (!argparse_dict_has_key(dict, key)) return cv_nil();
    return cool_dict_get(dict, cv_str(key));
}

static char* argparse_basename(const char* path) {
    if (!path || !*path) return strdup("program");
    const char* slash = strrchr(path, '/');
    const char* backslash = strrchr(path, '\\');
    const char* base = path;
    if (slash && backslash) {
        base = slash > backslash ? slash + 1 : backslash + 1;
    } else if (slash) {
        base = slash + 1;
    } else if (backslash) {
        base = backslash + 1;
    }
    if (!*base) return strdup("program");
    return strdup(base);
}

static char* argparse_default_prog_name(void) {
    if (COOL_SCRIPT_PATH && *COOL_SCRIPT_PATH) {
        return argparse_basename(COOL_SCRIPT_PATH);
    }
    CoolVal argv = cool_make_argv();
    CoolList* list = (CoolList*)(intptr_t)argv.payload;
    if (list && list->length > 0) {
        CoolVal first = ((CoolVal*)list->data)[0];
        if (first.tag == TAG_STR) return argparse_basename((const char*)(intptr_t)first.payload);
    }
    return strdup("program");
}

static char* argparse_normalize_long_flag(const char* src) {
    if (!src || !*src) argparse_failf("argparse option long flag cannot be empty");
    if (src[0] == '-' && src[1] == '-') return strdup(src);
    size_t len = strlen(src);
    char* out = (char*)malloc(len + 3);
    out[0] = '-';
    out[1] = '-';
    for (size_t i = 0; i < len; i++) {
        out[i + 2] = src[i] == '_' ? '-' : src[i];
    }
    out[len + 2] = '\0';
    return out;
}

static char* argparse_normalize_short_flag(const char* src) {
    if (!src || !*src) argparse_failf("argparse short option cannot be empty");
    if (src[0] == '-' && src[1] && src[2] == '\0') return strdup(src);
    if (src[0] != '-' && src[1] == '\0') {
        char* out = (char*)malloc(3);
        out[0] = '-';
        out[1] = src[0];
        out[2] = '\0';
        return out;
    }
    argparse_failf("argparse short option '%s' must be a single-character flag", src);
    return strdup("-?");
}

static int argparse_parse_bool_literal(const char* raw, int* out) {
    if (!raw) return 0;
    if (strcmp(raw, "1") == 0 || strcmp(raw, "true") == 0 || strcmp(raw, "True") == 0 ||
        strcmp(raw, "TRUE") == 0 || strcmp(raw, "yes") == 0 || strcmp(raw, "Yes") == 0 ||
        strcmp(raw, "YES") == 0 || strcmp(raw, "on") == 0 || strcmp(raw, "On") == 0 ||
        strcmp(raw, "ON") == 0) {
        *out = 1;
        return 1;
    }
    if (strcmp(raw, "0") == 0 || strcmp(raw, "false") == 0 || strcmp(raw, "False") == 0 ||
        strcmp(raw, "FALSE") == 0 || strcmp(raw, "no") == 0 || strcmp(raw, "No") == 0 ||
        strcmp(raw, "NO") == 0 || strcmp(raw, "off") == 0 || strcmp(raw, "Off") == 0 ||
        strcmp(raw, "OFF") == 0) {
        *out = 0;
        return 1;
    }
    return 0;
}

static CoolArgType argparse_parse_type(CoolVal value) {
    if (value.tag == TAG_NIL) return COOL_ARG_STR;
    if (value.tag != TAG_STR) {
        argparse_failf("argparse spec type must be a string, got %s", cool_to_str(value));
    }
    const char* s = (const char*)(intptr_t)value.payload;
    if (strcmp(s, "str") == 0 || strcmp(s, "string") == 0) return COOL_ARG_STR;
    if (strcmp(s, "int") == 0) return COOL_ARG_INT;
    if (strcmp(s, "float") == 0) return COOL_ARG_FLOAT;
    if (strcmp(s, "bool") == 0) return COOL_ARG_BOOL;
    argparse_failf("argparse spec type must be one of str/int/float/bool, got '%s'", s);
    return COOL_ARG_STR;
}

static CoolVal argparse_normalize_default(CoolArgType type, CoolVal value, const char* context) {
    switch (type) {
        case COOL_ARG_STR:
            if (value.tag == TAG_STR || value.tag == TAG_NIL) return value;
            argparse_failf("%s default must be a string or nil, got %s", context, cool_to_str(value));
            break;
        case COOL_ARG_INT:
            if (value.tag == TAG_INT || value.tag == TAG_NIL) return value;
            argparse_failf("%s default must be an int or nil, got %s", context, cool_to_str(value));
            break;
        case COOL_ARG_FLOAT:
            if (value.tag == TAG_FLOAT || value.tag == TAG_NIL) return value;
            if (value.tag == TAG_INT) return cv_float((double)value.payload);
            argparse_failf("%s default must be a float/int or nil, got %s", context, cool_to_str(value));
            break;
        case COOL_ARG_BOOL:
            if (value.tag == TAG_BOOL) return value;
            argparse_failf("%s default must be a bool, got %s", context, cool_to_str(value));
            break;
    }
    return value;
}

static CoolVal argparse_convert_text(CoolArgType type, const char* raw, const char* context) {
    char* end = NULL;
    errno = 0;
    switch (type) {
        case COOL_ARG_STR:
            return cv_str(strdup(raw));
        case COOL_ARG_INT: {
            long long value = strtoll(raw, &end, 10);
            if (errno != 0 || !end || *end != '\0') {
                argparse_failf("%s expects an int, got '%s'", context, raw);
            }
            return cv_int((int64_t)value);
        }
        case COOL_ARG_FLOAT: {
            double value = strtod(raw, &end);
            if (errno != 0 || !end || *end != '\0') {
                argparse_failf("%s expects a float, got '%s'", context, raw);
            }
            return cv_float(value);
        }
        case COOL_ARG_BOOL: {
            int b = 0;
            if (!argparse_parse_bool_literal(raw, &b)) {
                argparse_failf("%s expects a bool, got '%s'", context, raw);
            }
            return cv_bool(b);
        }
    }
    return cv_nil();
}

static const CoolArgOption* argparse_find_long_option(const CoolArgParser* parser, const char* flag, int* out_idx) {
    for (int i = 0; i < parser->option_count; i++) {
        if (strcmp(parser->options[i].long_flag, flag) == 0) {
            if (out_idx) *out_idx = i;
            return &parser->options[i];
        }
    }
    return NULL;
}

static const CoolArgOption* argparse_find_short_option(const CoolArgParser* parser, const char* flag, int* out_idx) {
    for (int i = 0; i < parser->option_count; i++) {
        if (parser->options[i].short_flag && strcmp(parser->options[i].short_flag, flag) == 0) {
            if (out_idx) *out_idx = i;
            return &parser->options[i];
        }
    }
    return NULL;
}

static CoolArgParser argparse_parse_spec(CoolVal spec_val) {
    if (spec_val.tag != TAG_DICT) argparse_failf("argparse spec must be a dict");
    CoolArgParser parser;
    memset(&parser, 0, sizeof(parser));

    CoolVal prog_val = argparse_dict_get_opt(spec_val, "prog");
    if (prog_val.tag == TAG_NIL) {
        parser.prog = argparse_default_prog_name();
    } else if (prog_val.tag == TAG_STR) {
        parser.prog = strdup((const char*)(intptr_t)prog_val.payload);
    } else {
        argparse_failf("argparse spec field 'prog' must be a string, got %s", cool_to_str(prog_val));
    }

    CoolVal desc_val = argparse_dict_get_opt(spec_val, "description");
    if (desc_val.tag == TAG_STR) {
        parser.description = strdup((const char*)(intptr_t)desc_val.payload);
    } else if (desc_val.tag != TAG_NIL) {
        argparse_failf(
            "argparse spec field 'description' must be a string, got %s",
            cool_to_str(desc_val)
        );
    }

    CoolVal positionals_val = argparse_dict_get_opt(spec_val, "positionals");
    if (positionals_val.tag != TAG_NIL) {
        if (!argparse_is_sequence(positionals_val)) {
            argparse_failf("argparse spec field 'positionals' must be a list or tuple");
        }
        CoolList* seq = (CoolList*)(intptr_t)positionals_val.payload;
        parser.positional_count = (int)seq->length;
        parser.positionals = (CoolArgPositional*)calloc((size_t)parser.positional_count, sizeof(CoolArgPositional));
        for (int i = 0; i < parser.positional_count; i++) {
            CoolVal entry = ((CoolVal*)seq->data)[i];
            if (entry.tag != TAG_DICT) argparse_failf("argparse positional entries must be dicts");
            CoolVal name_val = argparse_dict_get_opt(entry, "name");
            if (name_val.tag != TAG_STR) {
                argparse_failf("argparse positional field 'name' must be a string, got %s", cool_to_str(name_val));
            }
            const char* raw_name = (const char*)(intptr_t)name_val.payload;
            if (!*raw_name) argparse_failf("argparse positional name cannot be empty");
            for (int j = 0; j < i; j++) {
                if (strcmp(parser.positionals[j].name, raw_name) == 0) {
                    argparse_failf("argparse positional name '%s' is duplicated", raw_name);
                }
            }
            parser.positionals[i].name = strdup(raw_name);
            parser.positionals[i].arg_type = argparse_parse_type(argparse_dict_get_opt(entry, "type"));
            CoolVal help_val = argparse_dict_get_opt(entry, "help");
            if (help_val.tag == TAG_STR) {
                parser.positionals[i].help = strdup((const char*)(intptr_t)help_val.payload);
            } else if (help_val.tag != TAG_NIL) {
                argparse_failf(
                    "argparse positional '%s' field 'help' must be a string, got %s",
                    raw_name,
                    cool_to_str(help_val)
                );
            }
            CoolVal default_val = argparse_dict_get_opt(entry, "default");
            if (default_val.tag != TAG_NIL || argparse_dict_has_key(entry, "default")) {
                char ctx[256];
                snprintf(ctx, sizeof(ctx), "argparse positional '%s'", raw_name);
                parser.positionals[i].default_value =
                    argparse_normalize_default(parser.positionals[i].arg_type, default_val, ctx);
                parser.positionals[i].has_default = 1;
            }
            CoolVal required_val = argparse_dict_get_opt(entry, "required");
            if (required_val.tag == TAG_BOOL) {
                parser.positionals[i].required = required_val.payload ? 1 : 0;
            } else if (required_val.tag == TAG_NIL) {
                parser.positionals[i].required = parser.positionals[i].has_default ? 0 : 1;
            } else {
                argparse_failf(
                    "argparse positional '%s' field 'required' must be a bool, got %s",
                    raw_name,
                    cool_to_str(required_val)
                );
            }
        }
    }

    CoolVal options_val = argparse_dict_get_opt(spec_val, "options");
    if (options_val.tag != TAG_NIL) {
        if (!argparse_is_sequence(options_val)) {
            argparse_failf("argparse spec field 'options' must be a list or tuple");
        }
        CoolList* seq = (CoolList*)(intptr_t)options_val.payload;
        parser.option_count = (int)seq->length;
        parser.options = (CoolArgOption*)calloc((size_t)parser.option_count, sizeof(CoolArgOption));
        for (int i = 0; i < parser.option_count; i++) {
            CoolVal entry = ((CoolVal*)seq->data)[i];
            if (entry.tag != TAG_DICT) argparse_failf("argparse option entries must be dicts");
            CoolVal name_val = argparse_dict_get_opt(entry, "name");
            if (name_val.tag != TAG_STR) {
                argparse_failf("argparse option field 'name' must be a string, got %s", cool_to_str(name_val));
            }
            const char* raw_name = (const char*)(intptr_t)name_val.payload;
            if (!*raw_name) argparse_failf("argparse option name cannot be empty");
            for (int j = 0; j < i; j++) {
                if (strcmp(parser.options[j].name, raw_name) == 0) {
                    argparse_failf("argparse option name '%s' is duplicated", raw_name);
                }
            }
            parser.options[i].name = strdup(raw_name);
            parser.options[i].arg_type = argparse_parse_type(argparse_dict_get_opt(entry, "type"));
            CoolVal help_val = argparse_dict_get_opt(entry, "help");
            if (help_val.tag == TAG_STR) {
                parser.options[i].help = strdup((const char*)(intptr_t)help_val.payload);
            } else if (help_val.tag != TAG_NIL) {
                argparse_failf(
                    "argparse option '%s' field 'help' must be a string, got %s",
                    raw_name,
                    cool_to_str(help_val)
                );
            }
            CoolVal default_val = argparse_dict_get_opt(entry, "default");
            if (default_val.tag != TAG_NIL || argparse_dict_has_key(entry, "default")) {
                char ctx[256];
                snprintf(ctx, sizeof(ctx), "argparse option '%s'", raw_name);
                parser.options[i].default_value =
                    argparse_normalize_default(parser.options[i].arg_type, default_val, ctx);
                parser.options[i].has_default = 1;
            }
            CoolVal required_val = argparse_dict_get_opt(entry, "required");
            if (required_val.tag == TAG_BOOL) {
                parser.options[i].required = required_val.payload ? 1 : 0;
            } else if (required_val.tag == TAG_NIL) {
                parser.options[i].required = 0;
            } else {
                argparse_failf(
                    "argparse option '%s' field 'required' must be a bool, got %s",
                    raw_name,
                    cool_to_str(required_val)
                );
            }

            CoolVal long_val = argparse_dict_get_opt(entry, "long");
            parser.options[i].long_flag = argparse_normalize_long_flag(
                long_val.tag == TAG_STR ? (const char*)(intptr_t)long_val.payload : raw_name
            );
            for (int j = 0; j < i; j++) {
                if (strcmp(parser.options[j].long_flag, parser.options[i].long_flag) == 0) {
                    argparse_failf("argparse option flag '%s' is duplicated", parser.options[i].long_flag);
                }
            }

            CoolVal short_val = argparse_dict_get_opt(entry, "short");
            if (short_val.tag == TAG_STR) {
                parser.options[i].short_flag =
                    argparse_normalize_short_flag((const char*)(intptr_t)short_val.payload);
                for (int j = 0; j < i; j++) {
                    if (parser.options[j].short_flag &&
                        strcmp(parser.options[j].short_flag, parser.options[i].short_flag) == 0) {
                        argparse_failf("argparse option flag '%s' is duplicated", parser.options[i].short_flag);
                    }
                }
            } else if (short_val.tag != TAG_NIL) {
                argparse_failf(
                    "argparse option '%s' field 'short' must be a string, got %s",
                    raw_name,
                    cool_to_str(short_val)
                );
            }
        }
    }

    return parser;
}

static CoolVal argparse_option_value(
    const CoolArgOption* option,
    const char* inline_value,
    CoolVal argv_seq,
    int idx,
    int argc,
    int allow_next_bool_literal,
    int* consumed_next
) {
    char ctx[256];
    snprintf(ctx, sizeof(ctx), "argparse option '%s'", option->long_flag);
    *consumed_next = 0;

    if (option->arg_type == COOL_ARG_BOOL) {
        if (inline_value) return argparse_convert_text(option->arg_type, inline_value, ctx);
        if (allow_next_bool_literal && idx + 1 < argc) {
            CoolVal next = cool_list_get(argv_seq, cv_int(idx + 1));
            if (next.tag != TAG_STR) {
                argparse_failf("argparse.parse() argv items must be strings, got %s", cool_to_str(next));
            }
            int parsed = 0;
            if (argparse_parse_bool_literal((const char*)(intptr_t)next.payload, &parsed)) {
                *consumed_next = 1;
                return cv_bool(parsed);
            }
        }
        return cv_bool(1);
    }

    if (inline_value) return argparse_convert_text(option->arg_type, inline_value, ctx);
    if (idx + 1 >= argc) {
        argparse_failf("argparse.parse(): option '%s' requires a value", option->long_flag);
    }
    CoolVal next = cool_list_get(argv_seq, cv_int(idx + 1));
    if (next.tag != TAG_STR) {
        argparse_failf("argparse.parse() argv items must be strings, got %s", cool_to_str(next));
    }
    *consumed_next = 1;
    return argparse_convert_text(option->arg_type, (const char*)(intptr_t)next.payload, ctx);
}

static void argparse_help_row(
    CoolStrBuf* sb,
    const char* label,
    const char* help,
    int required,
    int show_default,
    CoolVal default_value,
    int show_false_default
) {
    sb_push_str(sb, "  ");
    sb_push_str(sb, label);
    size_t label_len = strlen(label);
    size_t pad = label_len < 24 ? 24 - label_len : 0;
    for (size_t i = 0; i < pad; i++) sb_push_char(sb, ' ');
    sb_push_char(sb, ' ');
    if (help && *help) sb_push_str(sb, help);
    int has_suffix = required || show_default || show_false_default;
    if (has_suffix) {
        if (help && *help) sb_push_char(sb, ' ');
        sb_push_char(sb, '(');
        int need_comma = 0;
        if (required) {
            sb_push_str(sb, "required");
            need_comma = 1;
        }
        if (show_default) {
            if (need_comma) sb_push_str(sb, ", ");
            sb_push_str(sb, "default: ");
            sb_push_str(sb, cool_to_str(default_value));
            need_comma = 1;
        }
        if (show_false_default) {
            if (need_comma) sb_push_str(sb, ", ");
            sb_push_str(sb, "default: false");
        }
        sb_push_char(sb, ')');
    }
    sb_push_char(sb, '\n');
}

static CoolVal cool_argparse_help(CoolVal spec_val) {
    CoolArgParser parser = argparse_parse_spec(spec_val);
    CoolStrBuf sb;
    sb_init(&sb);
    sb_push_str(&sb, "Usage: ");
    sb_push_str(&sb, parser.prog);

    for (int i = 0; i < parser.option_count; i++) {
        CoolArgOption* option = &parser.options[i];
        sb_push_char(&sb, ' ');
        if (!option->required) sb_push_char(&sb, '[');
        sb_push_str(&sb, option->long_flag);
        if (option->arg_type != COOL_ARG_BOOL) {
            sb_push_char(&sb, ' ');
            char* upper = strdup(option->name);
            for (char* p = upper; *p; p++) *p = (char)toupper((unsigned char)*p);
            sb_push_str(&sb, upper);
        }
        if (!option->required) sb_push_char(&sb, ']');
    }

    for (int i = 0; i < parser.positional_count; i++) {
        CoolArgPositional* positional = &parser.positionals[i];
        sb_push_char(&sb, ' ');
        if (!positional->required) sb_push_char(&sb, '[');
        char* upper = strdup(positional->name);
        for (char* p = upper; *p; p++) *p = (char)toupper((unsigned char)*p);
        sb_push_str(&sb, upper);
        if (!positional->required) sb_push_char(&sb, ']');
    }

    if (parser.description && *parser.description) {
        sb_push_str(&sb, "\n\n");
        sb_push_str(&sb, parser.description);
    }

    if (parser.positional_count > 0) {
        sb_push_str(&sb, "\n\nPositional arguments:\n");
        for (int i = 0; i < parser.positional_count; i++) {
            CoolArgPositional* positional = &parser.positionals[i];
            char* upper = strdup(positional->name);
            for (char* p = upper; *p; p++) *p = (char)toupper((unsigned char)*p);
            argparse_help_row(
                &sb,
                upper,
                positional->help,
                positional->required,
                positional->has_default,
                positional->default_value,
                0
            );
        }
    }

    if (parser.option_count > 0) {
        sb_push_str(&sb, "\n\nOptions:\n");
        for (int i = 0; i < parser.option_count; i++) {
            CoolArgOption* option = &parser.options[i];
            CoolStrBuf label;
            sb_init(&label);
            if (option->short_flag) {
                sb_push_str(&label, option->short_flag);
                sb_push_str(&label, ", ");
            }
            sb_push_str(&label, option->long_flag);
            if (option->arg_type != COOL_ARG_BOOL) {
                sb_push_char(&label, ' ');
                char* upper = strdup(option->name);
                for (char* p = upper; *p; p++) *p = (char)toupper((unsigned char)*p);
                sb_push_str(&label, upper);
            }
            argparse_help_row(
                &sb,
                label.data,
                option->help,
                option->required,
                option->has_default,
                option->default_value,
                option->arg_type == COOL_ARG_BOOL && !option->has_default
            );
        }
    }

    return cv_str(sb.data);
}

static CoolVal cool_argparse_parse(CoolVal spec_val, int has_argv, CoolVal argv_val) {
    CoolArgParser parser = argparse_parse_spec(spec_val);
    CoolVal result = cool_dict_new();
    for (int i = 0; i < parser.positional_count; i++) {
        cool_setindex(
            result,
            cv_str(parser.positionals[i].name),
            parser.positionals[i].has_default ? parser.positionals[i].default_value : cv_nil()
        );
    }
    for (int i = 0; i < parser.option_count; i++) {
        CoolVal value = cv_nil();
        if (parser.options[i].has_default) {
            value = parser.options[i].default_value;
        } else if (parser.options[i].arg_type == COOL_ARG_BOOL) {
            value = cv_bool(0);
        }
        cool_setindex(result, cv_str(parser.options[i].name), value);
    }

    CoolVal argv_seq = has_argv ? argv_val : cool_make_argv();
    if (!argparse_is_sequence(argv_seq)) {
        argparse_failf("argparse.parse() argv must be a list or tuple of strings, got %s", cool_to_str(argv_seq));
    }
    CoolList* argv_list = (CoolList*)(intptr_t)argv_seq.payload;
    int argc = (int)argv_list->length;
    int idx = has_argv ? 0 : (argc > 0 ? 1 : 0);
    int* seen_options = parser.option_count > 0 ? (int*)calloc((size_t)parser.option_count, sizeof(int)) : NULL;
    CoolVal positional_tokens = cool_list_make(cv_int(argc > 0 ? argc : 1));

    while (idx < argc) {
        CoolVal token_val = cool_list_get(argv_seq, cv_int(idx));
        if (token_val.tag != TAG_STR) {
            argparse_failf("argparse.parse() argv items must be strings, got %s", cool_to_str(token_val));
        }
        const char* token = (const char*)(intptr_t)token_val.payload;
        if (strcmp(token, "--") == 0) {
            for (int j = idx + 1; j < argc; j++) {
                CoolVal rest = cool_list_get(argv_seq, cv_int(j));
                if (rest.tag != TAG_STR) {
                    argparse_failf("argparse.parse() argv items must be strings, got %s", cool_to_str(rest));
                }
                cool_list_push(positional_tokens, rest);
            }
            break;
        }
        if (token[0] == '-' && token[1] == '-' && token[2] != '\0') {
            const char* inline_value = NULL;
            char* flag_name = NULL;
            const char* eq = strchr(token + 2, '=');
            if (eq) {
                size_t flag_len = (size_t)(eq - token);
                flag_name = (char*)malloc(flag_len + 1);
                memcpy(flag_name, token, flag_len);
                flag_name[flag_len] = '\0';
                inline_value = eq + 1;
            } else {
                flag_name = strdup(token);
            }
            int option_index = -1;
            const CoolArgOption* option = argparse_find_long_option(&parser, flag_name, &option_index);
            if (!option) argparse_failf("argparse.parse(): unknown option '%s'", token);
            int consumed_next = 0;
            CoolVal value = argparse_option_value(option, inline_value, argv_seq, idx, argc, 1, &consumed_next);
            cool_setindex(result, cv_str(option->name), value);
            if (option_index >= 0) seen_options[option_index] = 1;
            idx += consumed_next ? 2 : 1;
            continue;
        }
        if (token[0] == '-' && token[1] != '\0') {
            size_t cluster_len = strlen(token + 1);
            int consumed_next = 0;
            for (size_t pos = 0; pos < cluster_len; pos++) {
                char short_flag[3];
                short_flag[0] = '-';
                short_flag[1] = token[pos + 1];
                short_flag[2] = '\0';
                int option_index = -1;
                const CoolArgOption* option = argparse_find_short_option(&parser, short_flag, &option_index);
                if (!option) argparse_failf("argparse.parse(): unknown option '%s'", short_flag);
                if (option->arg_type == COOL_ARG_BOOL && pos + 1 < cluster_len) {
                    cool_setindex(result, cv_str(option->name), cv_bool(1));
                    if (option_index >= 0) seen_options[option_index] = 1;
                    continue;
                }
                const char* trailing = NULL;
                if (option->arg_type != COOL_ARG_BOOL && pos + 1 < cluster_len) {
                    trailing = token + pos + 2;
                }
                CoolVal value =
                    argparse_option_value(option, trailing, argv_seq, idx, argc, trailing == NULL, &consumed_next);
                cool_setindex(result, cv_str(option->name), value);
                if (option_index >= 0) seen_options[option_index] = 1;
                if (option->arg_type != COOL_ARG_BOOL) break;
            }
            idx += consumed_next ? 2 : 1;
            continue;
        }
        cool_list_push(positional_tokens, token_val);
        idx++;
    }

    CoolList* positional_list = (CoolList*)(intptr_t)positional_tokens.payload;
    if ((int)positional_list->length > parser.positional_count) {
        CoolVal extra = ((CoolVal*)positional_list->data)[parser.positional_count];
        argparse_failf(
            "argparse.parse(): unexpected positional argument '%s'",
            extra.tag == TAG_STR ? (const char*)(intptr_t)extra.payload : cool_to_str(extra)
        );
    }

    for (int i = 0; i < (int)positional_list->length; i++) {
        CoolVal raw = ((CoolVal*)positional_list->data)[i];
        if (raw.tag != TAG_STR) {
            argparse_failf("argparse.parse() argv items must be strings, got %s", cool_to_str(raw));
        }
        char ctx[256];
        snprintf(ctx, sizeof(ctx), "argparse positional '%s'", parser.positionals[i].name);
        CoolVal value =
            argparse_convert_text(parser.positionals[i].arg_type, (const char*)(intptr_t)raw.payload, ctx);
        cool_setindex(result, cv_str(parser.positionals[i].name), value);
    }

    for (int i = (int)positional_list->length; i < parser.positional_count; i++) {
        if (parser.positionals[i].required && !parser.positionals[i].has_default) {
            argparse_failf("argparse.parse(): missing required positional '%s'", parser.positionals[i].name);
        }
    }

    for (int i = 0; i < parser.option_count; i++) {
        if (parser.options[i].required && !seen_options[i]) {
            argparse_failf("argparse.parse(): missing required option '%s'", parser.options[i].long_flag);
        }
    }

    return result;
}

static uint64_t cool_rng_state = 88172645463325252ull;

static uint64_t cool_rng_next_u64(void) {
    uint64_t x = cool_rng_state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    cool_rng_state = x ? x : 1;
    return cool_rng_state;
}

static double cool_rng_next_f64(void) {
    uint64_t bits = cool_rng_next_u64();
    return (double)(bits >> 11) / (double)(1ull << 53);
}

static void sb_init(CoolStrBuf* sb) {
    sb->cap = 64;
    sb->len = 0;
    sb->data = (char*)malloc(sb->cap);
    sb->data[0] = '\0';
}

static void sb_reserve(CoolStrBuf* sb, size_t extra) {
    size_t need = sb->len + extra + 1;
    if (need <= sb->cap) return;
    while (sb->cap < need) sb->cap *= 2;
    sb->data = (char*)realloc(sb->data, sb->cap);
}

static void sb_push_char(CoolStrBuf* sb, char c) {
    sb_reserve(sb, 1);
    sb->data[sb->len++] = c;
    sb->data[sb->len] = '\0';
}

static void sb_push_str(CoolStrBuf* sb, const char* s) {
    size_t n = strlen(s);
    sb_reserve(sb, n);
    memcpy(sb->data + sb->len, s, n);
    sb->len += n;
    sb->data[sb->len] = '\0';
}

static int cool_mkdir_p(const char* path) {
    if (!path || !*path) return 0;
    char* copy = strdup(path);
    if (!copy) return -1;
    size_t len = strlen(copy);
    if (len > 1 && copy[len - 1] == '/') copy[len - 1] = '\0';
    for (char* p = copy + 1; *p; p++) {
        if (*p == '/') {
            *p = '\0';
            if (mkdir(copy, 0777) != 0 && errno != EEXIST) {
                free(copy);
                return -1;
            }
            *p = '/';
        }
    }
    if (mkdir(copy, 0777) != 0 && errno != EEXIST) {
        free(copy);
        return -1;
    }
    free(copy);
    return 0;
}

static void sb_push_json_escaped(CoolStrBuf* sb, const char* s) {
    sb_push_char(sb, '"');
    for (const unsigned char* p = (const unsigned char*)s; *p; p++) {
        switch (*p) {
            case '\\': sb_push_str(sb, "\\\\"); break;
            case '"': sb_push_str(sb, "\\\""); break;
            case '\n': sb_push_str(sb, "\\n"); break;
            case '\r': sb_push_str(sb, "\\r"); break;
            case '\t': sb_push_str(sb, "\\t"); break;
            default: sb_push_char(sb, (char)*p); break;
        }
    }
    sb_push_char(sb, '"');
}

static void json_skip_ws(const char** p) {
    while (**p && isspace((unsigned char)**p)) (*p)++;
}

static char* json_parse_string_raw(const char** p) {
    if (**p != '"') {
        fprintf(stderr, "ValueError: json.loads() expected string\n");
        exit(1);
    }
    (*p)++;
    CoolStrBuf sb;
    sb_init(&sb);
    while (**p && **p != '"') {
        unsigned char ch = (unsigned char)**p;
        if (ch == '\\') {
            (*p)++;
            switch (**p) {
                case '"': sb_push_char(&sb, '"'); break;
                case '\\': sb_push_char(&sb, '\\'); break;
                case '/': sb_push_char(&sb, '/'); break;
                case 'b': sb_push_char(&sb, '\b'); break;
                case 'f': sb_push_char(&sb, '\f'); break;
                case 'n': sb_push_char(&sb, '\n'); break;
                case 'r': sb_push_char(&sb, '\r'); break;
                case 't': sb_push_char(&sb, '\t'); break;
                default:
                    fprintf(stderr, "ValueError: json.loads() unsupported escape\n");
                    exit(1);
            }
        } else {
            sb_push_char(&sb, (char)ch);
        }
        (*p)++;
    }
    if (**p != '"') {
        fprintf(stderr, "ValueError: json.loads() unterminated string\n");
        exit(1);
    }
    (*p)++;
    return sb.data;
}

static CoolVal json_parse_value(const char** p);

static CoolVal json_parse_array(const char** p) {
    (*p)++;
    json_skip_ws(p);
    CoolVal out = cool_list_make(cv_int(4));
    if (**p == ']') {
        (*p)++;
        return out;
    }
    while (1) {
        cool_list_push(out, json_parse_value(p));
        json_skip_ws(p);
        if (**p == ']') {
            (*p)++;
            return out;
        }
        if (**p != ',') {
            fprintf(stderr, "ValueError: json.loads() expected ',' or ']'\n");
            exit(1);
        }
        (*p)++;
        json_skip_ws(p);
    }
}

static CoolVal json_parse_object(const char** p) {
    (*p)++;
    json_skip_ws(p);
    CoolVal out = cool_dict_new();
    if (**p == '}') {
        (*p)++;
        return out;
    }
    while (1) {
        json_skip_ws(p);
        char* key = json_parse_string_raw(p);
        json_skip_ws(p);
        if (**p != ':') {
            fprintf(stderr, "ValueError: json.loads() expected ':'\n");
            exit(1);
        }
        (*p)++;
        json_skip_ws(p);
        out = cool_dict_set(out, cv_str(key), json_parse_value(p));
        json_skip_ws(p);
        if (**p == '}') {
            (*p)++;
            return out;
        }
        if (**p != ',') {
            fprintf(stderr, "ValueError: json.loads() expected ',' or '}'\n");
            exit(1);
        }
        (*p)++;
        json_skip_ws(p);
    }
}

static CoolVal json_parse_number(const char** p) {
    char* end = NULL;
    double f = strtod(*p, &end);
    if (end == *p) {
        fprintf(stderr, "ValueError: json.loads() expected number\n");
        exit(1);
    }
    int is_float = 0;
    for (const char* q = *p; q < end; q++) {
        if (*q == '.' || *q == 'e' || *q == 'E') {
            is_float = 1;
            break;
        }
    }
    *p = end;
    return is_float ? cv_float(f) : cv_int((int64_t)f);
}

static CoolVal json_parse_value(const char** p) {
    json_skip_ws(p);
    switch (**p) {
        case '"': return cv_str(json_parse_string_raw(p));
        case '[': return json_parse_array(p);
        case '{': return json_parse_object(p);
        case 't':
            if (strncmp(*p, "true", 4) == 0) { *p += 4; return cv_bool(1); }
            break;
        case 'f':
            if (strncmp(*p, "false", 5) == 0) { *p += 5; return cv_bool(0); }
            break;
        case 'n':
            if (strncmp(*p, "null", 4) == 0) { *p += 4; return cv_nil(); }
            break;
        default:
            if (**p == '-' || isdigit((unsigned char)**p)) return json_parse_number(p);
            break;
    }
    fprintf(stderr, "ValueError: json.loads() invalid JSON\n");
    exit(1);
}

static void json_dump_value(CoolStrBuf* sb, CoolVal v) {
    switch (v.tag) {
        case TAG_NIL: sb_push_str(sb, "null"); break;
        case TAG_BOOL: sb_push_str(sb, v.payload ? "true" : "false"); break;
        case TAG_INT: {
            char buf[64];
            snprintf(buf, sizeof(buf), "%lld", (long long)v.payload);
            sb_push_str(sb, buf);
            break;
        }
        case TAG_FLOAT: {
            char buf[64];
            snprintf(buf, sizeof(buf), "%g", cv_as_float(v));
            sb_push_str(sb, buf);
            break;
        }
        case TAG_STR:
            sb_push_json_escaped(sb, (const char*)(intptr_t)v.payload);
            break;
        case TAG_LIST:
        case TAG_TUPLE: {
            CoolList* lst = (CoolList*)(intptr_t)v.payload;
            sb_push_char(sb, '[');
            for (int64_t i = 0; i < lst->length; i++) {
                if (i > 0) sb_push_str(sb, ", ");
                json_dump_value(sb, ((CoolVal*)lst->data)[i]);
            }
            sb_push_char(sb, ']');
            break;
        }
        case TAG_DICT: {
            CoolDict* d = (CoolDict*)(intptr_t)v.payload;
            sb_push_char(sb, '{');
            for (int64_t i = 0; i < d->len; i++) {
                if (i > 0) sb_push_str(sb, ", ");
                if (d->keys[i].tag == TAG_STR) {
                    sb_push_json_escaped(sb, (const char*)(intptr_t)d->keys[i].payload);
                } else {
                    char* key = cool_to_str(d->keys[i]);
                    sb_push_json_escaped(sb, key);
                }
                sb_push_str(sb, ": ");
                json_dump_value(sb, d->vals[i]);
            }
            sb_push_char(sb, '}');
            break;
        }
        default: {
            char* s = cool_to_str(v);
            sb_push_json_escaped(sb, s);
            break;
        }
    }
}

static char* re_translate_pattern(const char* pattern) {
    CoolStrBuf sb;
    sb_init(&sb);
    for (const char* p = pattern; *p; p++) {
        if (*p == '\\' && p[1]) {
            p++;
            switch (*p) {
                case 'd': sb_push_str(&sb, "[[:digit:]]"); break;
                case 'D': sb_push_str(&sb, "[^[:digit:]]"); break;
                case 's': sb_push_str(&sb, "[[:space:]]"); break;
                case 'S': sb_push_str(&sb, "[^[:space:]]"); break;
                case 'w': sb_push_str(&sb, "[[:alnum:]_]"); break;
                case 'W': sb_push_str(&sb, "[^[:alnum:]_]"); break;
                default:
                    sb_push_char(&sb, '\\');
                    sb_push_char(&sb, *p);
                    break;
            }
        } else {
            sb_push_char(&sb, *p);
        }
    }
    return sb.data;
}

static regex_t re_compile_regex(const char* pattern) {
    char* translated = re_translate_pattern(pattern);
    regex_t re;
    int rc = regcomp(&re, translated, REG_EXTENDED);
    free(translated);
    if (rc != 0) {
        char errbuf[256];
        regerror(rc, &re, errbuf, sizeof(errbuf));
        fprintf(stderr, "ValueError: invalid regex: %s\n", errbuf);
        exit(1);
    }
    return re;
}

static char* cool_strdup_range(const char* start, size_t len) {
    char* out = (char*)malloc(len + 1);
    memcpy(out, start, len);
    out[len] = '\0';
    return out;
}

static char* cool_path_join(int32_t nargs, CoolVal* args) {
    if (nargs == 0) return strdup("");
    char* out = strdup("");
    size_t out_len = 0;
    for (int32_t i = 0; i < nargs; i++) {
        const char* part = cool_to_str(args[i]);
        if (part[0] == '/') {
            free(out);
            out = strdup(part);
            out_len = strlen(out);
            continue;
        }
        size_t part_len = strlen(part);
        size_t need_sep = (out_len > 0 && out[out_len - 1] != '/') ? 1 : 0;
        out = (char*)realloc(out, out_len + need_sep + part_len + 1);
        if (need_sep) out[out_len++] = '/';
        memcpy(out + out_len, part, part_len);
        out_len += part_len;
        out[out_len] = '\0';
    }
    return out;
}

static char* cool_path_basename_str(const char* path) {
    const char* end = path + strlen(path);
    while (end > path && end[-1] == '/') end--;
    if (end == path) return strdup("");
    const char* base = end;
    while (base > path && base[-1] != '/') base--;
    return cool_strdup_range(base, (size_t)(end - base));
}

static char* cool_path_dirname_str(const char* path) {
    const char* end = path + strlen(path);
    while (end > path + 1 && end[-1] == '/') end--;
    const char* base = end;
    while (base > path && base[-1] != '/') base--;
    if (base == path) {
        if (path[0] == '/') return strdup("/");
        return strdup("");
    }
    return cool_strdup_range(path, (size_t)(base - path - 1));
}

static char* cool_path_ext_str(const char* path) {
    char* base = cool_path_basename_str(path);
    char* dot = strrchr(base, '.');
    if (!dot || dot == base) {
        free(base);
        return strdup("");
    }
    char* out = strdup(dot);
    free(base);
    return out;
}

static char* cool_path_stem_str(const char* path) {
    char* base = cool_path_basename_str(path);
    char* dot = strrchr(base, '.');
    if (!dot || dot == base) return base;
    *dot = '\0';
    return base;
}

static char* cool_path_normalize_str(const char* path) {
    int is_abs = path[0] == '/';
    char* copy = strdup(path);
    char* save = NULL;
    char* parts[256];
    int count = 0;
    for (char* tok = strtok_r(copy, "/", &save); tok; tok = strtok_r(NULL, "/", &save)) {
        if (strcmp(tok, ".") == 0 || tok[0] == '\0') continue;
        if (strcmp(tok, "..") == 0) {
            if (count > 0 && strcmp(parts[count - 1], "..") != 0) count--;
            else if (!is_abs) parts[count++] = tok;
            continue;
        }
        parts[count++] = tok;
    }
    if (count == 0) {
        free(copy);
        return strdup(is_abs ? "/" : ".");
    }
    size_t len = is_abs ? 1 : 0;
    for (int i = 0; i < count; i++) len += strlen(parts[i]) + (i + 1 < count ? 1 : 0);
    char* out = (char*)malloc(len + 1);
    char* p = out;
    if (is_abs) *p++ = '/';
    for (int i = 0; i < count; i++) {
        size_t n = strlen(parts[i]);
        memcpy(p, parts[i], n);
        p += n;
        if (i + 1 < count) *p++ = '/';
    }
    *p = '\0';
    free(copy);
    return out;
}

static CoolVal cool_path_split_val(const char* path) {
    CoolVal out = cool_list_make(cv_int(2));
    cool_list_push(out, cv_str(cool_path_dirname_str(path)));
    cool_list_push(out, cv_str(cool_path_basename_str(path)));
    return out;
}

enum {
    COOL_LOG_DEBUG = 0,
    COOL_LOG_INFO = 1,
    COOL_LOG_WARNING = 2,
    COOL_LOG_ERROR = 3
};

static int g_logging_level = COOL_LOG_INFO;
static int g_logging_timestamp = 0;
static int g_logging_stdout = 1;
static char* g_logging_format = NULL;
static char* g_logging_file = NULL;
static int g_logging_file_needs_reset = 0;

static void cool_logging_raisef(const char* fmt, ...) {
    char buf[512];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    cool_raise(cv_str(strdup(buf)));
}

static int cool_str_ieq(const char* a, const char* b) {
    while (*a && *b) {
        if (tolower((unsigned char)*a) != tolower((unsigned char)*b)) return 0;
        a++;
        b++;
    }
    return *a == '\0' && *b == '\0';
}

static const char* cool_logging_level_name(int level) {
    switch (level) {
        case COOL_LOG_DEBUG: return "DEBUG";
        case COOL_LOG_INFO: return "INFO";
        case COOL_LOG_WARNING: return "WARNING";
        case COOL_LOG_ERROR: return "ERROR";
        default: return "INFO";
    }
}

static int cool_logging_parse_level(const char* raw) {
    if (cool_str_ieq(raw, "debug")) return COOL_LOG_DEBUG;
    if (cool_str_ieq(raw, "info")) return COOL_LOG_INFO;
    if (cool_str_ieq(raw, "warning") || cool_str_ieq(raw, "warn")) return COOL_LOG_WARNING;
    if (cool_str_ieq(raw, "error")) return COOL_LOG_ERROR;
    cool_logging_raisef("logging level must be one of DEBUG/INFO/WARNING/ERROR, got '%s'", raw);
    return COOL_LOG_INFO;
}

static int cool_logging_dict_has(CoolVal dict, const char* key) {
    return cool_truthy(cool_dict_contains(dict, cv_str(key)));
}

static CoolVal cool_logging_dict_get(CoolVal dict, const char* key) {
    return cool_dict_get_opt(dict, cv_str(key));
}

static void cool_logging_reset_defaults(void) {
    g_logging_level = COOL_LOG_INFO;
    g_logging_timestamp = 0;
    g_logging_stdout = 1;
    if (g_logging_format) free(g_logging_format);
    if (g_logging_file) free(g_logging_file);
    g_logging_format = NULL;
    g_logging_file = NULL;
    g_logging_file_needs_reset = 0;
}

static void cool_logging_apply_config(CoolVal config) {
    if (config.tag == TAG_NIL) {
        cool_logging_reset_defaults();
        return;
    }
    if (config.tag != TAG_DICT) {
        cool_logging_raisef("logging.basic_config() expects a config dict, got %s", cool_to_str(config));
    }

    CoolDict* dict = (CoolDict*)(intptr_t)config.payload;
    for (int64_t i = 0; i < dict->len; i++) {
        CoolVal key = dict->keys[i];
        if (key.tag != TAG_STR) {
            cool_logging_raisef("logging.basic_config() keys must be strings, got %s", cool_to_str(key));
        }
        const char* key_s = (const char*)(intptr_t)key.payload;
        if (strcmp(key_s, "level") != 0 &&
            strcmp(key_s, "format") != 0 &&
            strcmp(key_s, "timestamp") != 0 &&
            strcmp(key_s, "stdout") != 0 &&
            strcmp(key_s, "file") != 0 &&
            strcmp(key_s, "append") != 0) {
            cool_logging_raisef("logging.basic_config() does not support field '%s'", key_s);
        }
    }

    int new_level = COOL_LOG_INFO;
    int new_timestamp = 0;
    int new_stdout = 1;
    int new_append = 0;
    char* new_format = NULL;
    char* new_file = NULL;

    if (cool_logging_dict_has(config, "level")) {
        CoolVal value = cool_logging_dict_get(config, "level");
        if (value.tag != TAG_STR) {
            cool_logging_raisef("logging.basic_config() field 'level' must be a string, got %s", cool_to_str(value));
        }
        new_level = cool_logging_parse_level((const char*)(intptr_t)value.payload);
    }
    if (cool_logging_dict_has(config, "format")) {
        CoolVal value = cool_logging_dict_get(config, "format");
        if (value.tag == TAG_NIL) {
            new_format = NULL;
        } else if (value.tag == TAG_STR) {
            new_format = strdup((const char*)(intptr_t)value.payload);
        } else {
            cool_logging_raisef(
                "logging.basic_config() field 'format' must be a string or nil, got %s",
                cool_to_str(value)
            );
        }
    }
    if (cool_logging_dict_has(config, "timestamp")) {
        CoolVal value = cool_logging_dict_get(config, "timestamp");
        if (value.tag == TAG_NIL) {
            new_timestamp = 0;
        } else if (value.tag == TAG_BOOL) {
            new_timestamp = value.payload != 0;
        } else {
            cool_logging_raisef(
                "logging.basic_config() field 'timestamp' must be a bool, got %s",
                cool_to_str(value)
            );
        }
    }
    if (cool_logging_dict_has(config, "stdout")) {
        CoolVal value = cool_logging_dict_get(config, "stdout");
        if (value.tag == TAG_NIL) {
            new_stdout = 0;
        } else if (value.tag == TAG_BOOL) {
            new_stdout = value.payload != 0;
        } else {
            cool_logging_raisef(
                "logging.basic_config() field 'stdout' must be a bool, got %s",
                cool_to_str(value)
            );
        }
    }
    if (cool_logging_dict_has(config, "file")) {
        CoolVal value = cool_logging_dict_get(config, "file");
        if (value.tag == TAG_NIL) {
            new_file = NULL;
        } else if (value.tag == TAG_STR) {
            const char* path = (const char*)(intptr_t)value.payload;
            if (!*path) {
                cool_logging_raisef("logging.basic_config() field 'file' cannot be empty");
            }
            new_file = strdup(path);
        } else {
            cool_logging_raisef(
                "logging.basic_config() field 'file' must be a string or nil, got %s",
                cool_to_str(value)
            );
        }
    }
    if (cool_logging_dict_has(config, "append")) {
        CoolVal value = cool_logging_dict_get(config, "append");
        if (value.tag == TAG_NIL) {
            new_append = 0;
        } else if (value.tag == TAG_BOOL) {
            new_append = value.payload != 0;
        } else {
            cool_logging_raisef(
                "logging.basic_config() field 'append' must be a bool, got %s",
                cool_to_str(value)
            );
        }
    }

    if (g_logging_format) free(g_logging_format);
    if (g_logging_file) free(g_logging_file);
    g_logging_level = new_level;
    g_logging_timestamp = new_timestamp;
    g_logging_stdout = new_stdout;
    g_logging_format = new_format;
    g_logging_file = new_file;
    g_logging_file_needs_reset = (new_file && !new_append) ? 1 : 0;
}

static char* cool_logging_render_line(int level, const char* message, const char* logger_name) {
    const char* level_name = cool_logging_level_name(level);
    const char* name = logger_name ? logger_name : "";
    char timestamp_buf[32];
    int needs_timestamp = g_logging_timestamp || (g_logging_format && strstr(g_logging_format, "{timestamp}") != NULL);
    if (needs_timestamp) {
        snprintf(timestamp_buf, sizeof(timestamp_buf), "%lld", (long long)time(NULL));
    } else {
        timestamp_buf[0] = '\0';
    }

    CoolStrBuf sb;
    sb_init(&sb);
    if (g_logging_format) {
        const char* p = g_logging_format;
        while (*p) {
            if (strncmp(p, "{timestamp}", 11) == 0) {
                sb_push_str(&sb, timestamp_buf);
                p += 11;
            } else if (strncmp(p, "{level}", 7) == 0) {
                sb_push_str(&sb, level_name);
                p += 7;
            } else if (strncmp(p, "{name}", 6) == 0) {
                sb_push_str(&sb, name);
                p += 6;
            } else if (strncmp(p, "{message}", 9) == 0) {
                sb_push_str(&sb, message);
                p += 9;
            } else {
                sb_push_char(&sb, *p);
                p++;
            }
        }
        return sb.data;
    }

    if (g_logging_timestamp) {
        sb_push_str(&sb, timestamp_buf);
        sb_push_char(&sb, ' ');
    }
    sb_push_char(&sb, '[');
    sb_push_str(&sb, level_name);
    sb_push_str(&sb, "] ");
    if (name[0]) {
        sb_push_str(&sb, name);
        sb_push_str(&sb, ": ");
    }
    sb_push_str(&sb, message);
    return sb.data;
}

static CoolVal cool_logging_emit(int level, const char* message, const char* logger_name) {
    if (level < g_logging_level) return cv_nil();

    char* line = cool_logging_render_line(level, message, logger_name);
    if (g_logging_stdout) {
        fprintf(stdout, "%s\n", line);
        fflush(stdout);
    }
    if (g_logging_file) {
        const char* mode = g_logging_file_needs_reset ? "w" : "a";
        FILE* fp = fopen(g_logging_file, mode);
        if (!fp) {
            cool_logging_raisef("logging file error for '%s': %s", g_logging_file, strerror(errno));
        }
        fprintf(fp, "%s\n", line);
        fclose(fp);
        g_logging_file_needs_reset = 0;
    }
    free(line);
    return cv_nil();
}

enum {
    FFI_T_VOID = 0,
    FFI_T_I8,
    FFI_T_I16,
    FFI_T_I32,
    FFI_T_I64,
    FFI_T_U8,
    FFI_T_U16,
    FFI_T_U32,
    FFI_T_U64,
    FFI_T_F32,
    FFI_T_F64,
    FFI_T_PTR,
    FFI_T_STR
};

typedef struct {
    int is_float;
    int64_t i;
    double f;
} CoolFfiSlot;

static int cool_ffi_is_float_type(int32_t ty) {
    return ty == FFI_T_F32 || ty == FFI_T_F64;
}

static const char* cool_ffi_type_name(int32_t ty) {
    switch (ty) {
        case FFI_T_VOID: return "void";
        case FFI_T_I8: return "i8";
        case FFI_T_I16: return "i16";
        case FFI_T_I32: return "i32";
        case FFI_T_I64: return "i64";
        case FFI_T_U8: return "u8";
        case FFI_T_U16: return "u16";
        case FFI_T_U32: return "u32";
        case FFI_T_U64: return "u64";
        case FFI_T_F32: return "f32";
        case FFI_T_F64: return "f64";
        case FFI_T_PTR: return "ptr";
        case FFI_T_STR: return "str";
        default: return "<unknown>";
    }
}

static int32_t cool_ffi_parse_type(const char* name) {
    if (strcmp(name, "void") == 0) return FFI_T_VOID;
    if (strcmp(name, "i8") == 0) return FFI_T_I8;
    if (strcmp(name, "i16") == 0) return FFI_T_I16;
    if (strcmp(name, "i32") == 0) return FFI_T_I32;
    if (strcmp(name, "i64") == 0) return FFI_T_I64;
    if (strcmp(name, "u8") == 0) return FFI_T_U8;
    if (strcmp(name, "u16") == 0) return FFI_T_U16;
    if (strcmp(name, "u32") == 0) return FFI_T_U32;
    if (strcmp(name, "u64") == 0) return FFI_T_U64;
    if (strcmp(name, "f32") == 0) return FFI_T_F32;
    if (strcmp(name, "f64") == 0) return FFI_T_F64;
    if (strcmp(name, "ptr") == 0) return FFI_T_PTR;
    if (strcmp(name, "str") == 0) return FFI_T_STR;
    fprintf(stderr, "ValueError: unknown FFI type '%s'\n", name);
    exit(1);
}

static int64_t cool_ffi_value_to_i64(CoolVal v, int arg_index, const char* ty_name) {
    switch (v.tag) {
        case TAG_INT:
            return v.payload;
        case TAG_BOOL:
            return v.payload ? 1 : 0;
        case TAG_FLOAT:
            return (int64_t)cv_as_float(v);
        default:
            fprintf(stderr, "TypeError: FFI arg %d cannot convert %s to %s\n", arg_index, cool_type_name(v.tag), ty_name);
            exit(1);
    }
}

static double cool_ffi_value_to_f64(CoolVal v, int arg_index, const char* ty_name) {
    switch (v.tag) {
        case TAG_FLOAT:
            return cv_as_float(v);
        case TAG_INT:
            return (double)v.payload;
        case TAG_BOOL:
            return v.payload ? 1.0 : 0.0;
        default:
            fprintf(stderr, "TypeError: FFI arg %d cannot convert %s to %s\n", arg_index, cool_type_name(v.tag), ty_name);
            exit(1);
    }
}

static CoolFfiSlot cool_ffi_value_to_slot(CoolVal v, int32_t ty, char** owned_strings, int arg_index) {
    CoolFfiSlot slot;
    slot.is_float = 0;
    slot.i = 0;
    slot.f = 0.0;

    if (ty == FFI_T_F32 || ty == FFI_T_F64) {
        slot.is_float = 1;
        slot.f = cool_ffi_value_to_f64(v, arg_index, cool_ffi_type_name(ty));
        return slot;
    }

    if (ty == FFI_T_STR) {
        const char* src = NULL;
        if (v.tag == TAG_STR) src = (const char*)(intptr_t)v.payload;
        else if (v.tag == TAG_NIL) src = "";
        else src = cool_to_str(v);
        owned_strings[arg_index] = strdup(src ? src : "");
        if (!owned_strings[arg_index]) {
            fprintf(stderr, "RuntimeError: out of memory preparing FFI string argument\n");
            exit(1);
        }
        slot.i = (int64_t)(intptr_t)owned_strings[arg_index];
        return slot;
    }

    slot.i = cool_ffi_value_to_i64(v, arg_index, cool_ffi_type_name(ty));
    return slot;
}

static CoolVal cool_ffi_int_return(int64_t raw, int32_t ret_type) {
    switch (ret_type) {
        case FFI_T_I8: return cv_int((int8_t)raw);
        case FFI_T_I16: return cv_int((int16_t)raw);
        case FFI_T_I32: return cv_int((int32_t)raw);
        case FFI_T_U8: return cv_int((uint8_t)raw);
        case FFI_T_U16: return cv_int((uint16_t)raw);
        case FFI_T_U32: return cv_int((uint32_t)raw);
        default: return cv_int(raw);
    }
}

static void* cool_ffi_try_open(const char* candidate) {
    if (!candidate || !*candidate) return NULL;
    dlerror();
    return dlopen(candidate, RTLD_LAZY);
}

static void* cool_ffi_open_library(const char* name, const char** resolved_name) {
    if (!name || !*name) return NULL;

    if (strchr(name, '/') || strchr(name, '.')) {
        void* handle = cool_ffi_try_open(name);
        if (handle) {
            if (resolved_name) *resolved_name = name;
            return handle;
        }
    } else {
        char a[512], b[512], c[512], d[512], e[512], f[512], g[512], h[512];
#if defined(__APPLE__)
        snprintf(a, sizeof(a), "lib%s.dylib", name);
        snprintf(b, sizeof(b), "%s.dylib", name);
        snprintf(c, sizeof(c), "/usr/lib/lib%s.dylib", name);
        snprintf(d, sizeof(d), "/usr/local/lib/lib%s.dylib", name);
        snprintf(e, sizeof(e), "/opt/homebrew/lib/lib%s.dylib", name);
        const char* candidates[] = { a, b, c, d, e, NULL };
#elif defined(__linux__)
        snprintf(a, sizeof(a), "lib%s.so", name);
        snprintf(b, sizeof(b), "%s.so", name);
        snprintf(c, sizeof(c), "lib%s.so.6", name);
        snprintf(d, sizeof(d), "%s.so.6", name);
        snprintf(e, sizeof(e), "/usr/lib/lib%s.so", name);
        snprintf(f, sizeof(f), "/usr/local/lib/lib%s.so", name);
        snprintf(g, sizeof(g), "/lib/x86_64-linux-gnu/lib%s.so.6", name);
        snprintf(h, sizeof(h), "/lib/aarch64-linux-gnu/lib%s.so.6", name);
        const char* candidates[] = { a, b, c, d, e, f, g, h, NULL };
#else
        const char* candidates[] = { name, NULL };
#endif
        for (int i = 0; candidates[i] != NULL; i++) {
            void* handle = cool_ffi_try_open(candidates[i]);
            if (handle) {
                if (resolved_name) *resolved_name = candidates[i];
                return handle;
            }
        }
    }

    return NULL;
}

CoolVal cool_ffi_open(CoolVal name_v) {
    if (name_v.tag != TAG_STR) {
        fprintf(stderr, "TypeError: ffi.open() requires a string path\n");
        exit(1);
    }
    const char* requested = (const char*)(intptr_t)name_v.payload;
    const char* resolved = requested;
    void* handle = cool_ffi_open_library(requested, &resolved);
    if (!handle) {
        const char* err = dlerror();
        fprintf(stderr, "RuntimeError: ffi.open(\"%s\") failed: %s\n", requested, err ? err : "unknown error");
        exit(1);
    }
    CoolFfiLib* lib = (CoolFfiLib*)malloc(sizeof(CoolFfiLib));
    if (!lib) {
        fprintf(stderr, "RuntimeError: out of memory opening ffi library\n");
        exit(1);
    }
    lib->tag = TAG_FFI_LIB;
    lib->handle = handle;
    (void)resolved;
    CoolVal out;
    out.tag = TAG_FFI_LIB;
    out.payload = (int64_t)(intptr_t)lib;
    return out;
}

CoolVal cool_ffi_func(CoolVal lib_v, CoolVal name_v, CoolVal ret_type_v, CoolVal arg_types_v) {
    if (lib_v.tag != TAG_FFI_LIB) {
        fprintf(stderr, "TypeError: ffi.func(): first argument must be an ffi library\n");
        exit(1);
    }
    if (name_v.tag != TAG_STR) {
        fprintf(stderr, "TypeError: ffi.func(): second argument must be a string\n");
        exit(1);
    }
    if (ret_type_v.tag != TAG_STR) {
        fprintf(stderr, "TypeError: ffi.func(): third argument must be a type string\n");
        exit(1);
    }

    CoolFfiLib* lib = (CoolFfiLib*)(intptr_t)lib_v.payload;
    const char* sym_name = (const char*)(intptr_t)name_v.payload;
    const char* ret_name = (const char*)(intptr_t)ret_type_v.payload;

    int32_t argc = 0;
    int32_t arg_types[8] = {0};
    if (arg_types_v.tag != TAG_NIL) {
        if (arg_types_v.tag != TAG_LIST && arg_types_v.tag != TAG_TUPLE) {
            fprintf(stderr, "TypeError: ffi.func(): fourth argument must be a list\n");
            exit(1);
        }
        CoolList* list = (CoolList*)(intptr_t)arg_types_v.payload;
        if (list->length > 8) {
            fprintf(stderr, "ValueError: ffi.func(): supports at most 8 arguments\n");
            exit(1);
        }
        for (int64_t i = 0; i < list->length; i++) {
            CoolVal item = ((CoolVal*)list->data)[i];
            if (item.tag != TAG_STR) {
                fprintf(stderr, "TypeError: ffi.func(): arg_types list must contain strings\n");
                exit(1);
            }
            arg_types[argc++] = cool_ffi_parse_type((const char*)(intptr_t)item.payload);
        }
    }

    dlerror();
    void* sym = dlsym(lib->handle, sym_name);
    const char* err = dlerror();
    if (!sym || err) {
        fprintf(stderr, "RuntimeError: ffi.func(): symbol '%s' not found: %s\n", sym_name, err ? err : "unknown error");
        exit(1);
    }

    CoolFfiFunc* fn = (CoolFfiFunc*)malloc(sizeof(CoolFfiFunc));
    if (!fn) {
        fprintf(stderr, "RuntimeError: out of memory creating ffi function\n");
        exit(1);
    }
    fn->tag = TAG_FFI_FUNC;
    fn->handle = lib->handle;
    fn->sym = sym;
    fn->name = strdup(sym_name);
    fn->ret_type = cool_ffi_parse_type(ret_name);
    fn->argc = argc;
    for (int i = 0; i < 8; i++) fn->arg_types[i] = (i < argc) ? arg_types[i] : FFI_T_VOID;

    CoolVal out;
    out.tag = TAG_FFI_FUNC;
    out.payload = (int64_t)(intptr_t)fn;
    return out;
}

static CoolVal cool_ffi_dispatch(CoolFfiFunc* fn, CoolFfiSlot* slots) {
    int n = fn->argc;
    int32_t ret = fn->ret_type;
    void* sym = fn->sym;
#define FFI_AS_I(idx) ((slots[idx].is_float) ? (int64_t)slots[idx].f : slots[idx].i)
#define FFI_AS_F64(idx) ((slots[idx].is_float) ? slots[idx].f : (double)slots[idx].i)
#define FFI_AS_F32(idx) ((slots[idx].is_float) ? (float)slots[idx].f : (float)slots[idx].i)

    if (n == 0) {
        switch (ret) {
            case FFI_T_VOID:
                ((void (*)(void))sym)();
                return cv_nil();
            case FFI_T_F32:
                return cv_float((double)((float (*)(void))sym)());
            case FFI_T_F64:
                return cv_float(((double (*)(void))sym)());
            case FFI_T_STR: {
                const char* out = ((const char* (*)(void))sym)();
                return out ? cv_str(strdup(out)) : cv_nil();
            }
            default:
                return cool_ffi_int_return(((int64_t (*)(void))sym)(), ret);
        }
    }

    if (n == 1) {
        int32_t t0 = fn->arg_types[0];
        if (t0 == FFI_T_F64) {
            double a = FFI_AS_F64(0);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(double))sym)(a); return cv_nil();
                case FFI_T_F32: return cv_float((double)((float (*)(double))sym)(a));
                case FFI_T_F64: return cv_float(((double (*)(double))sym)(a));
                case FFI_T_STR: {
                    const char* out = ((const char* (*)(double))sym)(a);
                    return out ? cv_str(strdup(out)) : cv_nil();
                }
                default: return cool_ffi_int_return(((int64_t (*)(double))sym)(a), ret);
            }
        } else if (t0 == FFI_T_F32) {
            float a = FFI_AS_F32(0);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(float))sym)(a); return cv_nil();
                case FFI_T_F32: return cv_float((double)((float (*)(float))sym)(a));
                case FFI_T_F64: return cv_float(((double (*)(float))sym)(a));
                default: return cool_ffi_int_return(((int64_t (*)(float))sym)(a), ret);
            }
        } else {
            int64_t a = FFI_AS_I(0);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(int64_t))sym)(a); return cv_nil();
                case FFI_T_F32: return cv_float((double)((float (*)(int64_t))sym)(a));
                case FFI_T_F64: return cv_float(((double (*)(int64_t))sym)(a));
                case FFI_T_STR: {
                    const char* out = ((const char* (*)(const char*))sym)((const char*)(intptr_t)a);
                    return out ? cv_str(strdup(out)) : cv_nil();
                }
                default: return cool_ffi_int_return(((int64_t (*)(int64_t))sym)(a), ret);
            }
        }
    }

    if (n == 2) {
        int32_t t0 = fn->arg_types[0];
        int32_t t1 = fn->arg_types[1];
        if (t0 == FFI_T_F64 && t1 == FFI_T_F64) {
            double a = FFI_AS_F64(0), b = FFI_AS_F64(1);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(double, double))sym)(a, b); return cv_nil();
                case FFI_T_F32: return cv_float((double)((float (*)(double, double))sym)(a, b));
                case FFI_T_F64: return cv_float(((double (*)(double, double))sym)(a, b));
                default: return cool_ffi_int_return(((int64_t (*)(double, double))sym)(a, b), ret);
            }
        } else if (t0 == FFI_T_F32 && t1 == FFI_T_F32) {
            float a = FFI_AS_F32(0), b = FFI_AS_F32(1);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(float, float))sym)(a, b); return cv_nil();
                case FFI_T_F32: return cv_float((double)((float (*)(float, float))sym)(a, b));
                case FFI_T_F64: return cv_float(((double (*)(float, float))sym)(a, b));
                default: return cool_ffi_int_return(((int64_t (*)(float, float))sym)(a, b), ret);
            }
        } else if (t0 == FFI_T_F64 && !cool_ffi_is_float_type(t1)) {
            double a = FFI_AS_F64(0);
            int64_t b = FFI_AS_I(1);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(double, int64_t))sym)(a, b); return cv_nil();
                case FFI_T_F64: return cv_float(((double (*)(double, int64_t))sym)(a, b));
                default: return cool_ffi_int_return(((int64_t (*)(double, int64_t))sym)(a, b), ret);
            }
        } else if (!cool_ffi_is_float_type(t0) && t1 == FFI_T_F64) {
            int64_t a = FFI_AS_I(0);
            double b = FFI_AS_F64(1);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(int64_t, double))sym)(a, b); return cv_nil();
                case FFI_T_F64: return cv_float(((double (*)(int64_t, double))sym)(a, b));
                default: return cool_ffi_int_return(((int64_t (*)(int64_t, double))sym)(a, b), ret);
            }
        } else {
            int64_t a = FFI_AS_I(0), b = FFI_AS_I(1);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(int64_t, int64_t))sym)(a, b); return cv_nil();
                case FFI_T_F32: return cv_float((double)((float (*)(int64_t, int64_t))sym)(a, b));
                case FFI_T_F64: return cv_float(((double (*)(int64_t, int64_t))sym)(a, b));
                default: return cool_ffi_int_return(((int64_t (*)(int64_t, int64_t))sym)(a, b), ret);
            }
        }
    }

    if (n == 3) {
        int32_t t0 = fn->arg_types[0];
        int32_t t1 = fn->arg_types[1];
        int32_t t2 = fn->arg_types[2];
        if (t0 == FFI_T_F64 && t1 == FFI_T_F64 && t2 == FFI_T_F64) {
            double a = FFI_AS_F64(0), b = FFI_AS_F64(1), c = FFI_AS_F64(2);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(double, double, double))sym)(a, b, c); return cv_nil();
                case FFI_T_F64: return cv_float(((double (*)(double, double, double))sym)(a, b, c));
                default: return cool_ffi_int_return(((int64_t (*)(double, double, double))sym)(a, b, c), ret);
            }
        }
        if (!cool_ffi_is_float_type(t0) && !cool_ffi_is_float_type(t1) && !cool_ffi_is_float_type(t2)) {
            int64_t a = FFI_AS_I(0), b = FFI_AS_I(1), c = FFI_AS_I(2);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(int64_t, int64_t, int64_t))sym)(a, b, c); return cv_nil();
                case FFI_T_F64: return cv_float(((double (*)(int64_t, int64_t, int64_t))sym)(a, b, c));
                default: return cool_ffi_int_return(((int64_t (*)(int64_t, int64_t, int64_t))sym)(a, b, c), ret);
            }
        }
    }

    if (n == 4) {
        int32_t t0 = fn->arg_types[0];
        int32_t t1 = fn->arg_types[1];
        int32_t t2 = fn->arg_types[2];
        int32_t t3 = fn->arg_types[3];
        if (t0 == FFI_T_F64 && t1 == FFI_T_F64 && t2 == FFI_T_F64 && t3 == FFI_T_F64) {
            double a = FFI_AS_F64(0), b = FFI_AS_F64(1), c = FFI_AS_F64(2), d = FFI_AS_F64(3);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(double, double, double, double))sym)(a, b, c, d); return cv_nil();
                case FFI_T_F64: return cv_float(((double (*)(double, double, double, double))sym)(a, b, c, d));
                default: return cool_ffi_int_return(((int64_t (*)(double, double, double, double))sym)(a, b, c, d), ret);
            }
        }
        if (!cool_ffi_is_float_type(t0) && !cool_ffi_is_float_type(t1) && !cool_ffi_is_float_type(t2) && !cool_ffi_is_float_type(t3)) {
            int64_t a = FFI_AS_I(0), b = FFI_AS_I(1), c = FFI_AS_I(2), d = FFI_AS_I(3);
            switch (ret) {
                case FFI_T_VOID: ((void (*)(int64_t, int64_t, int64_t, int64_t))sym)(a, b, c, d); return cv_nil();
                case FFI_T_F64: return cv_float(((double (*)(int64_t, int64_t, int64_t, int64_t))sym)(a, b, c, d));
                default: return cool_ffi_int_return(((int64_t (*)(int64_t, int64_t, int64_t, int64_t))sym)(a, b, c, d), ret);
            }
        }
    }

    fprintf(stderr, "RuntimeError: FFI unsupported call signature (%s", cool_ffi_type_name(fn->arg_types[0]));
    for (int i = 1; i < fn->argc; i++) fprintf(stderr, ", %s", cool_ffi_type_name(fn->arg_types[i]));
    fprintf(stderr, ") -> %s\n", cool_ffi_type_name(fn->ret_type));
    exit(1);
}

CoolVal cool_ffi_call(CoolVal fn_v, int32_t nargs, ...) {
    if (fn_v.tag != TAG_FFI_FUNC) {
        fprintf(stderr, "TypeError: value is not an ffi function\n");
        exit(1);
    }
    CoolFfiFunc* fn = (CoolFfiFunc*)(intptr_t)fn_v.payload;
    if (nargs != fn->argc) {
        fprintf(stderr, "RuntimeError: FFI call expected %d args, got %d\n", fn->argc, nargs);
        exit(1);
    }
    if (nargs > 8) {
        fprintf(stderr, "RuntimeError: FFI supports at most 8 arguments\n");
        exit(1);
    }

    va_list ap;
    va_start(ap, nargs);
    CoolVal argv[8];
    for (int32_t i = 0; i < nargs; i++) argv[i] = va_arg(ap, CoolVal);
    va_end(ap);

    char* owned_strings[8] = {0};
    CoolFfiSlot slots[8];
    for (int32_t i = 0; i < nargs; i++) {
        slots[i] = cool_ffi_value_to_slot(argv[i], fn->arg_types[i], owned_strings, i);
    }

    CoolVal result = cool_ffi_dispatch(fn, slots);
    for (int32_t i = 0; i < nargs; i++) {
        if (owned_strings[i]) free(owned_strings[i]);
    }
    return result;
}

int32_t cool_is_ffi_func(CoolVal v) {
    return v.tag == TAG_FFI_FUNC ? 1 : 0;
}

CoolVal cool_noncallable(CoolVal v) {
    fprintf(stderr, "TypeError: '%s' value is not callable\n", cool_type_name(v.tag));
    exit(1);
}

CoolVal cool_module_get_attr(const char* module, const char* name) {
    if (strcmp(module, "math") == 0) {
        if (strcmp(name, "pi") == 0) return cv_float(M_PI);
        if (strcmp(name, "e") == 0) return cv_float(M_E);
        if (strcmp(name, "tau") == 0) return cv_float(M_PI * 2.0);
        if (strcmp(name, "inf") == 0) return cv_float(INFINITY);
        if (strcmp(name, "nan") == 0) return cv_float(NAN);
    }
    if (strcmp(module, "sys") == 0) {
        if (strcmp(name, "argv") == 0) return cool_make_argv();
    }
    if (strcmp(module, "collections") == 0) {
        cool_init_collections_classes();
        if (strcmp(name, "Queue") == 0) return g_queue_class;
        if (strcmp(name, "Stack") == 0) return g_stack_class;
    }
    return cv_nil();
}

CoolVal cool_module_call(const char* module, const char* name, int32_t nargs, ...) {
    va_list ap;
    va_start(ap, nargs);
    CoolVal args[8];
    for (int32_t i = 0; i < nargs && i < 8; i++) args[i] = va_arg(ap, CoolVal);
    va_end(ap);

    if (strcmp(module, "math") == 0) {
        if (nargs == 1) {
            double x = cv_to_float(args[0]);
            if (strcmp(name, "sqrt") == 0) return cv_float(sqrt(x));
            if (strcmp(name, "floor") == 0) return cv_int((int64_t)floor(x));
            if (strcmp(name, "ceil") == 0) return cv_int((int64_t)ceil(x));
            if (strcmp(name, "round") == 0) return cool_round(args[0], cv_nil());
            if (strcmp(name, "sin") == 0) return cv_float(sin(x));
            if (strcmp(name, "cos") == 0) return cv_float(cos(x));
            if (strcmp(name, "tan") == 0) return cv_float(tan(x));
            if (strcmp(name, "asin") == 0) return cv_float(asin(x));
            if (strcmp(name, "acos") == 0) return cv_float(acos(x));
            if (strcmp(name, "atan") == 0) return cv_float(atan(x));
            if (strcmp(name, "log") == 0) return cv_float(log(x));
            if (strcmp(name, "log2") == 0) return cv_float(log2(x));
            if (strcmp(name, "log10") == 0) return cv_float(log10(x));
            if (strcmp(name, "exp") == 0) return cv_float(exp(x));
            if (strcmp(name, "exp2") == 0) return cv_float(exp2(x));
            if (strcmp(name, "degrees") == 0) return cv_float(x * 180.0 / M_PI);
            if (strcmp(name, "radians") == 0) return cv_float(x * M_PI / 180.0);
            if (strcmp(name, "sinh") == 0) return cv_float(sinh(x));
            if (strcmp(name, "cosh") == 0) return cv_float(cosh(x));
            if (strcmp(name, "tanh") == 0) return cv_float(tanh(x));
            if (strcmp(name, "round") == 0) return cool_round(args[0], cv_nil());
            if (strcmp(name, "trunc") == 0) return cv_int((int64_t)trunc(x));
            if (strcmp(name, "abs") == 0) return cool_abs(args[0]);
            if (strcmp(name, "isnan") == 0) return cv_bool(isnan(x));
            if (strcmp(name, "isinf") == 0) return cv_bool(isinf(x));
            if (strcmp(name, "isfinite") == 0) return cv_bool(isfinite(x));
            if (strcmp(name, "factorial") == 0) {
                int64_t n = cool_to_int(args[0]).payload;
                if (n < 0) {
                    fprintf(stderr, "ValueError: factorial() requires non-negative integer\n");
                    exit(1);
                }
                int64_t acc = 1;
                for (int64_t i = 2; i <= n; i++) acc *= i;
                return cv_int(acc);
            }
        }
        if (nargs == 2) {
            double x = cv_to_float(args[0]);
            double y = cv_to_float(args[1]);
            if (strcmp(name, "round") == 0) return cool_round(args[0], cool_to_int(args[1]));
            if (strcmp(name, "log") == 0) return cv_float(log(x) / log(y));
            if (strcmp(name, "pow") == 0) return cv_float(pow(x, y));
            if (strcmp(name, "atan2") == 0) return cv_float(atan2(x, y));
            if (strcmp(name, "hypot") == 0) return cv_float(hypot(x, y));
            if (strcmp(name, "gcd") == 0) {
                int64_t a = llabs(cool_to_int(args[0]).payload);
                int64_t b = llabs(cool_to_int(args[1]).payload);
                while (b != 0) { int64_t t = b; b = a % b; a = t; }
                return cv_int(a);
            }
            if (strcmp(name, "lcm") == 0) {
                int64_t a = cool_to_int(args[0]).payload;
                int64_t b = cool_to_int(args[1]).payload;
                int64_t aa = llabs(a), bb = llabs(b);
                int64_t g = aa;
                int64_t t = bb;
                while (t != 0) { int64_t n = t; t = g % t; g = n; }
                return cv_int((a == 0 || b == 0) ? 0 : llabs(a / g * b));
            }
        }
    }

    if (strcmp(module, "path") == 0) {
        if (strcmp(name, "join") == 0) return cv_str(cool_path_join(nargs, args));
        if (strcmp(name, "basename") == 0 && nargs == 1) return cv_str(cool_path_basename_str(cool_to_str(args[0])));
        if (strcmp(name, "dirname") == 0 && nargs == 1) return cv_str(cool_path_dirname_str(cool_to_str(args[0])));
        if (strcmp(name, "ext") == 0 && nargs == 1) return cv_str(cool_path_ext_str(cool_to_str(args[0])));
        if (strcmp(name, "stem") == 0 && nargs == 1) return cv_str(cool_path_stem_str(cool_to_str(args[0])));
        if (strcmp(name, "split") == 0 && nargs == 1) return cool_path_split_val(cool_to_str(args[0]));
        if (strcmp(name, "normalize") == 0 && nargs == 1) return cv_str(cool_path_normalize_str(cool_to_str(args[0])));
        if (strcmp(name, "exists") == 0 && nargs == 1) {
            struct stat st;
            return cv_bool(stat(cool_to_str(args[0]), &st) == 0);
        }
        if (strcmp(name, "isabs") == 0 && nargs == 1) {
            const char* path = cool_to_str(args[0]);
            return cv_bool(path[0] == '/');
        }
    }

    if (strcmp(module, "os") == 0) {
        if (strcmp(name, "getcwd") == 0 && nargs == 0) {
            char buf[4096];
            if (!getcwd(buf, sizeof(buf))) {
                fprintf(stderr, "RuntimeError: os.getcwd failed\n");
                exit(1);
            }
            return cv_str(strdup(buf));
        }
        if (strcmp(name, "exists") == 0 && nargs == 1) {
            const char* path = cool_to_str(args[0]);
            struct stat st;
            return cv_bool(stat(path, &st) == 0);
        }
        if (strcmp(name, "getenv") == 0 && nargs == 1) {
            const char* name_arg = cool_to_str(args[0]);
            const char* value = getenv(name_arg);
            if (!value) return cv_nil();
            return cv_str(strdup(value));
        }
        if (strcmp(name, "popen") == 0 && nargs == 1) {
            const char* cmd = cool_to_str(args[0]);
            FILE* pipe = popen(cmd, "r");
            if (!pipe) {
                fprintf(stderr, "RuntimeError: os.popen failed\n");
                exit(1);
            }
            CoolStrBuf sb;
            sb_init(&sb);
            char buf[1024];
            while (fgets(buf, sizeof(buf), pipe) != NULL) {
                sb_push_str(&sb, buf);
            }
            if (pclose(pipe) == -1) {
                fprintf(stderr, "RuntimeError: os.popen failed\n");
                exit(1);
            }
            return cv_str(sb.data);
        }
        if ((strcmp(name, "join") == 0 || strcmp(name, "path") == 0) && nargs >= 1) {
            size_t total = 1;
            for (int32_t i = 0; i < nargs; i++) total += strlen(cool_to_str(args[i])) + 1;
            char* out = (char*)malloc(total);
            char* p = out;
            for (int32_t i = 0; i < nargs; i++) {
                const char* part = cool_to_str(args[i]);
                size_t len = strlen(part);
                if (i > 0 && p > out && p[-1] != '/') *p++ = '/';
                memcpy(p, part, len);
                p += len;
            }
            *p = '\0';
            return cv_str(out);
        }
        if (strcmp(name, "listdir") == 0 && nargs == 1) {
            const char* path = cool_to_str(args[0]);
            DIR* dir = opendir(path);
            if (!dir) {
                fprintf(stderr, "RuntimeError: os.listdir failed\n");
                exit(1);
            }
            CoolVal out = cool_list_make(cv_int(8));
            struct dirent* ent;
            while ((ent = readdir(dir)) != NULL) {
                if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0) continue;
                cool_list_push(out, cv_str(strdup(ent->d_name)));
            }
            closedir(dir);
            return out;
        }
        if (strcmp(name, "mkdir") == 0 && nargs == 1) {
            const char* path = cool_to_str(args[0]);
            if (cool_mkdir_p(path) != 0) {
                fprintf(stderr, "RuntimeError: os.mkdir failed\n");
                exit(1);
            }
            return cv_nil();
        }
        if (strcmp(name, "remove") == 0 && nargs == 1) {
            const char* path = cool_to_str(args[0]);
            if (remove(path) != 0) {
                fprintf(stderr, "RuntimeError: os.remove failed\n");
                exit(1);
            }
            return cv_nil();
        }
        if (strcmp(name, "rename") == 0 && nargs == 2) {
            const char* src = cool_to_str(args[0]);
            const char* dst = cool_to_str(args[1]);
            if (rename(src, dst) != 0) {
                fprintf(stderr, "RuntimeError: os.rename failed\n");
                exit(1);
            }
            return cv_nil();
        }
    }

    if (strcmp(module, "sys") == 0) {
        if (strcmp(name, "exit") == 0 && nargs <= 1) {
            int code = 0;
            if (nargs == 1) code = (int)cool_to_int(args[0]).payload;
            exit(code);
        }
    }

    if (strcmp(module, "ffi") == 0) {
        if (strcmp(name, "open") == 0 && nargs == 1) {
            return cool_ffi_open(args[0]);
        }
        if (strcmp(name, "func") == 0 && (nargs == 3 || nargs == 4)) {
            return cool_ffi_func(args[0], args[1], args[2], nargs == 4 ? args[3] : cv_nil());
        }
    }

    if (strcmp(module, "subprocess") == 0) {
        if ((strcmp(name, "run") == 0 || strcmp(name, "call") == 0 || strcmp(name, "check_output") == 0)
            && (nargs == 1 || nargs == 2)) {
            const char* cmd = cool_to_str(args[0]);
            int has_timeout = 0;
            double timeout_secs = 0.0;
            if (nargs == 2 && args[1].tag != TAG_NIL) {
                has_timeout = 1;
                timeout_secs = cv_to_float(args[1]);
                if (timeout_secs < 0.0) timeout_secs = 0.0;
            }
            CoolSubprocessResult result = cool_subprocess_run_shell(cmd, has_timeout, timeout_secs);
            if (strcmp(name, "run") == 0) {
                return cool_subprocess_result_dict(result);
            }
            if (strcmp(name, "call") == 0) {
                return result.has_code ? cv_int(result.code) : cv_nil();
            }
            if (result.timed_out) {
                cool_raise(cv_str("subprocess.check_output() timed out"));
            }
            if (!result.has_code || result.code != 0) {
                CoolStrBuf sb;
                sb_init(&sb);
                sb_push_str(&sb, "subprocess.check_output() exited with code ");
                if (result.has_code) {
                    char code_buf[32];
                    snprintf(code_buf, sizeof(code_buf), "%d", result.code);
                    sb_push_str(&sb, code_buf);
                } else {
                    sb_push_str(&sb, "nil");
                }
                if (result.stderr_data && result.stderr_data[0] != '\0') {
                    sb_push_str(&sb, ": ");
                    sb_push_str(&sb, result.stderr_data);
                }
                cool_raise(cv_str(sb.data));
            }
            return cv_str(result.stdout_data);
        }
    }

    if (strcmp(module, "argparse") == 0) {
        if (strcmp(name, "parse") == 0 && (nargs == 1 || nargs == 2)) {
            return cool_argparse_parse(args[0], nargs == 2, nargs == 2 ? args[1] : cv_nil());
        }
        if (strcmp(name, "help") == 0 && nargs == 1) {
            return cool_argparse_help(args[0]);
        }
    }

    if (strcmp(module, "logging") == 0) {
        if (strcmp(name, "basic_config") == 0 && nargs <= 1) {
            cool_logging_apply_config(nargs == 1 ? args[0] : cv_nil());
            return cv_nil();
        }
        if (strcmp(name, "log") == 0 && (nargs == 2 || nargs == 3)) {
            if (args[0].tag != TAG_STR) {
                cool_logging_raisef("logging.log() level must be a string, got %s", cool_to_str(args[0]));
            }
            return cool_logging_emit(
                cool_logging_parse_level((const char*)(intptr_t)args[0].payload),
                cool_to_str(args[1]),
                nargs == 3 ? cool_to_str(args[2]) : NULL
            );
        }
        if ((strcmp(name, "debug") == 0 || strcmp(name, "info") == 0 || strcmp(name, "warning") == 0 ||
             strcmp(name, "warn") == 0 || strcmp(name, "error") == 0) && (nargs == 1 || nargs == 2)) {
            int level = COOL_LOG_INFO;
            if (strcmp(name, "debug") == 0) level = COOL_LOG_DEBUG;
            else if (strcmp(name, "warning") == 0 || strcmp(name, "warn") == 0) level = COOL_LOG_WARNING;
            else if (strcmp(name, "error") == 0) level = COOL_LOG_ERROR;
            return cool_logging_emit(level, cool_to_str(args[0]), nargs == 2 ? cool_to_str(args[1]) : NULL);
        }
    }

    if (strcmp(module, "csv") == 0) {
        if (strcmp(name, "rows") == 0 && nargs == 1) return cool_csv_rows(args[0]);
        if (strcmp(name, "dicts") == 0 && nargs == 1) return cool_csv_dicts(args[0]);
        if (strcmp(name, "write") == 0 && nargs == 1) return cool_csv_write(args[0]);
    }

    if (strcmp(module, "test") == 0) {
        if (strcmp(name, "equal") == 0 && (nargs == 2 || nargs == 3)) {
            if (!cool_truthy(cool_eq(args[0], args[1]))) {
                if (nargs == 3) return cool_test_raise_assertion(cool_to_str(args[2]));
                cool_test_raisef("expected %s == %s", cool_to_str(args[0]), cool_to_str(args[1]));
            }
            return cv_nil();
        }
        if (strcmp(name, "not_equal") == 0 && (nargs == 2 || nargs == 3)) {
            if (cool_truthy(cool_eq(args[0], args[1]))) {
                if (nargs == 3) return cool_test_raise_assertion(cool_to_str(args[2]));
                cool_test_raisef("expected %s != %s", cool_to_str(args[0]), cool_to_str(args[1]));
            }
            return cv_nil();
        }
        if ((strcmp(name, "true") == 0 || strcmp(name, "truthy") == 0) && (nargs == 1 || nargs == 2)) {
            if (!cool_truthy(args[0])) {
                if (nargs == 2) return cool_test_raise_assertion(cool_to_str(args[1]));
                return cool_test_raise_assertion("expected truthy value");
            }
            return cv_nil();
        }
        if ((strcmp(name, "false") == 0 || strcmp(name, "falsey") == 0) && (nargs == 1 || nargs == 2)) {
            if (cool_truthy(args[0])) {
                if (nargs == 2) return cool_test_raise_assertion(cool_to_str(args[1]));
                return cool_test_raise_assertion("expected falsey value");
            }
            return cv_nil();
        }
        if ((strcmp(name, "nil") == 0 || strcmp(name, "is_nil") == 0) && (nargs == 1 || nargs == 2)) {
            if (args[0].tag != TAG_NIL) {
                if (nargs == 2) return cool_test_raise_assertion(cool_to_str(args[1]));
                cool_test_raisef("expected nil, got %s", cool_to_str(args[0]));
            }
            return cv_nil();
        }
        if (strcmp(name, "not_nil") == 0 && (nargs == 1 || nargs == 2)) {
            if (args[0].tag == TAG_NIL) {
                if (nargs == 2) return cool_test_raise_assertion(cool_to_str(args[1]));
                return cool_test_raise_assertion("expected non-nil value");
            }
            return cv_nil();
        }
        if (strcmp(name, "fail") == 0 && nargs <= 1) {
            return cool_test_raise_assertion(nargs == 1 ? cool_to_str(args[0]) : "test.fail() called");
        }
        if (strcmp(name, "raises") == 0 && (nargs >= 1 && nargs <= 3)) {
            return cool_test_raises(args[0], nargs >= 2 ? args[1] : cv_nil(), nargs == 3 ? args[2] : cv_nil());
        }
    }

    if (strcmp(module, "time") == 0) {
        if (strcmp(name, "time") == 0 && nargs == 0) {
            struct timespec ts;
#if defined(CLOCK_REALTIME)
            clock_gettime(CLOCK_REALTIME, &ts);
            return cv_float((double)ts.tv_sec + (double)ts.tv_nsec / 1000000000.0);
#else
            return cv_float((double)time(NULL));
#endif
        }
        if (strcmp(name, "monotonic") == 0 && nargs == 0) {
            struct timespec ts;
#if defined(CLOCK_MONOTONIC)
            clock_gettime(CLOCK_MONOTONIC, &ts);
            return cv_float((double)ts.tv_sec + (double)ts.tv_nsec / 1000000000.0);
#else
            return cv_float((double)clock() / (double)CLOCKS_PER_SEC);
#endif
        }
        if (strcmp(name, "sleep") == 0 && nargs == 1) {
            double secs = cv_to_float(args[0]);
            if (secs < 0.0) secs = 0.0;
            struct timespec req;
            req.tv_sec = (time_t)secs;
            req.tv_nsec = (long)((secs - (double)req.tv_sec) * 1000000000.0);
            if (req.tv_nsec < 0) req.tv_nsec = 0;
            nanosleep(&req, NULL);
            return cv_nil();
        }
    }

    if (strcmp(module, "random") == 0) {
        if (strcmp(name, "random") == 0 && nargs == 0) {
            return cv_float(cool_rng_next_f64());
        }
        if (strcmp(name, "seed") == 0 && nargs == 1) {
            uint64_t seed = (uint64_t)cv_to_float(args[0]);
            cool_rng_state = seed ? seed : 1;
            return cv_nil();
        }
        if (strcmp(name, "randint") == 0 && nargs == 2) {
            int64_t a = cool_to_int(args[0]).payload;
            int64_t b = cool_to_int(args[1]).payload;
            if (a > b) {
                fprintf(stderr, "ValueError: random.randint() a must be <= b\n");
                exit(1);
            }
            uint64_t range = (uint64_t)(b - a + 1);
            return cv_int(a + (int64_t)(cool_rng_next_u64() % range));
        }
        if (strcmp(name, "uniform") == 0 && nargs == 2) {
            double a = cv_to_float(args[0]);
            double b = cv_to_float(args[1]);
            return cv_float(a + cool_rng_next_f64() * (b - a));
        }
        if (strcmp(name, "choice") == 0 && nargs == 1) {
            if (args[0].tag != TAG_LIST && args[0].tag != TAG_TUPLE) {
                fprintf(stderr, "TypeError: random.choice() requires a list or tuple\n");
                exit(1);
            }
            CoolList* seq = (CoolList*)(intptr_t)args[0].payload;
            if (seq->length == 0) {
                fprintf(stderr, "ValueError: random.choice() called on empty sequence\n");
                exit(1);
            }
            int64_t idx = (int64_t)(cool_rng_next_u64() % (uint64_t)seq->length);
            return ((CoolVal*)seq->data)[idx];
        }
        if (strcmp(name, "shuffle") == 0 && nargs == 1) {
            if (args[0].tag != TAG_LIST) {
                fprintf(stderr, "TypeError: random.shuffle() requires a list\n");
                exit(1);
            }
            CoolList* seq = (CoolList*)(intptr_t)args[0].payload;
            CoolVal* items = (CoolVal*)seq->data;
            for (int64_t i = seq->length - 1; i > 0; i--) {
                int64_t j = (int64_t)(cool_rng_next_u64() % (uint64_t)(i + 1));
                CoolVal tmp = items[i];
                items[i] = items[j];
                items[j] = tmp;
            }
            return cv_nil();
        }
    }

    if (strcmp(module, "json") == 0) {
        if (strcmp(name, "loads") == 0 && nargs == 1) {
            const char* src = cool_to_str(args[0]);
            const char* p = src;
            CoolVal out = json_parse_value(&p);
            json_skip_ws(&p);
            if (*p != '\0') {
                fprintf(stderr, "ValueError: json.loads() trailing characters\n");
                exit(1);
            }
            return out;
        }
        if (strcmp(name, "dumps") == 0 && nargs == 1) {
            CoolStrBuf sb;
            sb_init(&sb);
            json_dump_value(&sb, args[0]);
            return cv_str(sb.data);
        }
    }

    if (strcmp(module, "string") == 0) {
        if (strcmp(name, "split") == 0 && (nargs == 1 || nargs == 2)) {
            return cool_string_split(args[0], nargs == 2 ? args[1] : cv_nil());
        }
        if (strcmp(name, "join") == 0 && nargs == 2) return cool_string_join(args[0], args[1]);
        if (strcmp(name, "strip") == 0 && nargs == 1) return cool_string_strip(args[0]);
        if (strcmp(name, "lstrip") == 0 && nargs == 1) return cool_string_lstrip(args[0]);
        if (strcmp(name, "rstrip") == 0 && nargs == 1) return cool_string_rstrip(args[0]);
        if (strcmp(name, "upper") == 0 && nargs == 1) return cool_string_upper(args[0]);
        if (strcmp(name, "lower") == 0 && nargs == 1) return cool_string_lower(args[0]);
        if (strcmp(name, "replace") == 0 && nargs == 3) return cool_string_replace(args[0], args[1], args[2]);
        if (strcmp(name, "startswith") == 0 && nargs == 2) return cool_string_startswith(args[0], args[1]);
        if (strcmp(name, "endswith") == 0 && nargs == 2) return cool_string_endswith(args[0], args[1]);
        if (strcmp(name, "find") == 0 && nargs == 2) return cool_string_find(args[0], args[1]);
        if (strcmp(name, "count") == 0 && nargs == 2) return cool_string_count(args[0], args[1]);
        if (strcmp(name, "title") == 0 && nargs == 1) return cool_string_title(args[0]);
        if (strcmp(name, "capitalize") == 0 && nargs == 1) return cool_string_capitalize(args[0]);
        if (strcmp(name, "format") == 0 && nargs >= 1) return cool_string_format(args[0], nargs - 1, &args[1]);
    }

    if (strcmp(module, "list") == 0) {
        if (strcmp(name, "sort") == 0 && nargs == 1) return cool_sorted(args[0]);
        if (strcmp(name, "reverse") == 0 && nargs == 1) return cool_list_reverse_copy(args[0]);
        if (strcmp(name, "map") == 0 && nargs == 2) return cool_list_map_copy(args[0], args[1]);
        if (strcmp(name, "filter") == 0 && nargs == 2) return cool_list_filter_copy(args[0], args[1]);
        if (strcmp(name, "reduce") == 0 && nargs == 2) return cool_list_reduce_copy(args[0], args[1], cv_nil(), 0);
        if (strcmp(name, "reduce") == 0 && nargs == 3) return cool_list_reduce_copy(args[0], args[1], args[2], 1);
        if (strcmp(name, "flatten") == 0 && nargs == 1) return cool_list_flatten_copy(args[0]);
        if (strcmp(name, "unique") == 0 && nargs == 1) return cool_list_unique_copy(args[0]);
    }

    if (strcmp(module, "re") == 0) {
        if (nargs < 2 || args[0].tag != TAG_STR || args[1].tag != TAG_STR) {
            fprintf(stderr, "TypeError: re.%s() requires pattern and text strings\n", name);
            exit(1);
        }
        const char* pattern = (const char*)(intptr_t)args[0].payload;
        const char* text = (const char*)(intptr_t)args[1].payload;
        regex_t re = re_compile_regex(pattern);
        regmatch_t m;

        if (strcmp(name, "match") == 0) {
            int rc = regexec(&re, text, 1, &m, 0);
            regfree(&re);
            if (rc == 0 && m.rm_so == 0) {
                size_t len = (size_t)(m.rm_eo - m.rm_so);
                char* out = (char*)malloc(len + 1);
                memcpy(out, text + m.rm_so, len);
                out[len] = '\0';
                return cv_str(out);
            }
            return cv_nil();
        }

        if (strcmp(name, "search") == 0) {
            int rc = regexec(&re, text, 1, &m, 0);
            regfree(&re);
            if (rc == 0) {
                size_t len = (size_t)(m.rm_eo - m.rm_so);
                char* out = (char*)malloc(len + 1);
                memcpy(out, text + m.rm_so, len);
                out[len] = '\0';
                return cv_str(out);
            }
            return cv_nil();
        }

        if (strcmp(name, "fullmatch") == 0) {
            int rc = regexec(&re, text, 1, &m, 0);
            regfree(&re);
            if (rc == 0 && m.rm_so == 0 && (size_t)m.rm_eo == strlen(text)) {
                size_t len = (size_t)(m.rm_eo - m.rm_so);
                char* out = (char*)malloc(len + 1);
                memcpy(out, text + m.rm_so, len);
                out[len] = '\0';
                return cv_str(out);
            }
            return cv_nil();
        }

        if (strcmp(name, "findall") == 0) {
            CoolVal out = cool_list_make(cv_int(4));
            const char* cur = text;
            size_t offset = 0;
            while (regexec(&re, cur, 1, &m, 0) == 0) {
                if (m.rm_so < 0 || m.rm_eo < 0) break;
                size_t start = offset + (size_t)m.rm_so;
                size_t end = offset + (size_t)m.rm_eo;
                size_t len = end - start;
                char* part = (char*)malloc(len + 1);
                memcpy(part, text + start, len);
                part[len] = '\0';
                cool_list_push(out, cv_str(part));
                if (m.rm_eo == 0) {
                    cur++;
                    offset++;
                } else {
                    cur += m.rm_eo;
                    offset += (size_t)m.rm_eo;
                }
            }
            regfree(&re);
            return out;
        }

        if (strcmp(name, "split") == 0) {
            CoolVal out = cool_list_make(cv_int(4));
            const char* cur = text;
            size_t offset = 0;
            while (regexec(&re, cur, 1, &m, 0) == 0) {
                if (m.rm_so < 0 || m.rm_eo < 0) break;
                size_t start = offset;
                size_t end = offset + (size_t)m.rm_so;
                size_t len = end - start;
                char* part = (char*)malloc(len + 1);
                memcpy(part, text + start, len);
                part[len] = '\0';
                cool_list_push(out, cv_str(part));
                if (m.rm_eo == 0) {
                    cur++;
                    offset++;
                } else {
                    cur += m.rm_eo;
                    offset += (size_t)m.rm_eo;
                }
            }
            cool_list_push(out, cv_str(strdup(text + offset)));
            regfree(&re);
            return out;
        }

        if (strcmp(name, "sub") == 0 && nargs == 3 && args[2].tag == TAG_STR) {
            const char* repl = (const char*)(intptr_t)args[2].payload;
            CoolStrBuf sb;
            sb_init(&sb);
            const char* cur = text;
            size_t offset = 0;
            while (regexec(&re, cur, 1, &m, 0) == 0) {
                if (m.rm_so < 0 || m.rm_eo < 0) break;
                size_t start = offset;
                size_t end = offset + (size_t)m.rm_so;
                size_t len = end - start;
                sb_reserve(&sb, len + strlen(repl));
                memcpy(sb.data + sb.len, text + start, len);
                sb.len += len;
                sb.data[sb.len] = '\0';
                sb_push_str(&sb, repl);
                if (m.rm_eo == 0) {
                    sb_push_char(&sb, *cur);
                    cur++;
                    offset++;
                } else {
                    cur += m.rm_eo;
                    offset += (size_t)m.rm_eo;
                }
            }
            sb_push_str(&sb, text + offset);
            regfree(&re);
            return cv_str(sb.data);
        }

        regfree(&re);
    }

    if (strcmp(module, "collections") == 0) {
        cool_init_collections_classes();
        if (strcmp(name, "Queue") == 0 && nargs == 0) return collections_make_instance(g_queue_class);
        if (strcmp(name, "Stack") == 0 && nargs == 0) return collections_make_instance(g_stack_class);
    }

    fprintf(stderr, "AttributeError: unknown module call %s.%s\n", module, name);
    exit(1);
}

static CoolVal cool_list_contains_local(CoolVal list, CoolVal item) {
    if (list.tag != TAG_LIST && list.tag != TAG_TUPLE) return cv_bool(0);
    CoolList* l = (CoolList*)(intptr_t)list.payload;
    for (int64_t i = 0; i < l->length; i++)
        if (cv_eq_raw(((CoolVal*)l->data)[i], item)) return cv_bool(1);
    return cv_bool(0);
}

CoolVal cool_contains(CoolVal container, CoolVal item) {
    if (container.tag == TAG_LIST || container.tag == TAG_TUPLE) return cool_list_contains_local(container, item);
    if (container.tag == TAG_DICT) return cool_dict_contains(container, item);
    if (container.tag == TAG_STR && item.tag == TAG_STR) {
        const char* haystack = (const char*)(intptr_t)container.payload;
        const char* needle   = (const char*)(intptr_t)item.payload;
        return cv_bool(strstr(haystack, needle) != NULL);
    }
    fprintf(stderr, "TypeError: 'in' not supported for this type\n");
    exit(1);
}

/* ── Closure runtime ───────────────────────────────────────────────────── */

#include <setjmp.h>

/* Closure: captures enclosing variables */
typedef struct {
    int32_t tag;
    int64_t fn_ptr;       /* pointer to the compiled function */
    int64_t num_captures; /* number of captured variables */
    CoolVal captures[];    /* captured CoolVal variables */
} CoolClosure;

/* Create a closure with n captured variables */
CoolVal cool_closure_new(int64_t fn_ptr, int64_t num_captures, CoolVal* captures) {
    CoolClosure* clo = (CoolClosure*)malloc(sizeof(CoolClosure) + num_captures * sizeof(CoolVal));
    clo->tag = TAG_CLOSURE;
    clo->fn_ptr = fn_ptr;
    clo->num_captures = num_captures;
    for (int64_t i = 0; i < num_captures; i++) {
        clo->captures[i] = captures[i];
    }
    CoolVal v; v.tag = TAG_CLOSURE; v.payload = (int64_t)(intptr_t)clo;
    return v;
}

/* Get the function pointer from a closure */
int64_t cool_closure_get_fn_ptr(CoolVal clo) {
    if (clo.tag != TAG_CLOSURE) { fprintf(stderr, "TypeError: not a closure\n"); exit(1); }
    CoolClosure* c = (CoolClosure*)(intptr_t)clo.payload;
    return c->fn_ptr;
}

/* Get captured variable from a closure */
CoolVal cool_closure_get_capture(CoolVal clo, int64_t idx) {
    if (clo.tag != TAG_CLOSURE) { fprintf(stderr, "TypeError: not a closure\n"); exit(1); }
    CoolClosure* c = (CoolClosure*)(intptr_t)clo.payload;
    if (idx < 0 || idx >= c->num_captures) { fprintf(stderr, "IndexError: closure capture index out of range\n"); exit(1); }
    return c->captures[idx];
}

/* Get number of captures */
int64_t cool_closure_get_num_captures(CoolVal clo) {
    if (clo.tag != TAG_CLOSURE) { fprintf(stderr, "TypeError: not a closure\n"); exit(1); }
    CoolClosure* c = (CoolClosure*)(intptr_t)clo.payload;
    return c->num_captures;
}

/* Check if a value is a closure */
int32_t cool_is_closure(CoolVal v) {
    return v.tag == TAG_CLOSURE ? 1 : 0;
}

/* Global for passing closure captures */
static CoolVal g_closure_captures[64];
static int g_num_closure_captures = 0;

void cool_set_closure_capture(int32_t idx, CoolVal val) {
    if (idx >= 0 && idx < 64) {
        g_closure_captures[idx] = val;
        if (idx >= g_num_closure_captures) g_num_closure_captures = idx + 1;
    }
}

CoolVal cool_get_closure_capture(int32_t idx) {
    if (idx >= 0 && idx < g_num_closure_captures) {
        return g_closure_captures[idx];
    }
    return cv_nil();
}

int32_t cool_get_num_closure_captures(void) {
    return g_num_closure_captures;
}

/* ── Exception handling with setjmp/longjmp ────────────────────────────── */

#define MAX_EXCEPTION_FRAMES 16
#define MAX_WITH_MANAGERS 64

typedef struct {
    void* buf;
    int active;
    int with_depth;
} ExceptionFrame;
static ExceptionFrame g_exception_frames[MAX_EXCEPTION_FRAMES];
static int g_exception_frame_count = 0;
static CoolVal g_current_exception;
static CoolVal g_with_managers[MAX_WITH_MANAGERS];
static int g_with_manager_count = 0;

static void cool_call_with_exit(CoolVal manager) {
    cool_call_method_vararg(manager, "method___exit__", 3, cv_nil(), cv_nil(), cv_nil());
}

void cool_push_with(CoolVal manager) {
    if (g_with_manager_count >= MAX_WITH_MANAGERS) {
        fprintf(stderr, "RuntimeError: too many nested with blocks\n");
        exit(1);
    }
    g_with_managers[g_with_manager_count++] = manager;
}

void cool_pop_with(void) {
    if (g_with_manager_count > 0) {
        g_with_manager_count--;
    }
}

static void cool_unwind_withs_to(int depth) {
    while (g_with_manager_count > depth) {
        CoolVal manager = g_with_managers[--g_with_manager_count];
        cool_call_with_exit(manager);
    }
}

/* Push an exception frame for a caller-owned jmp_buf. */
void cool_enter_try(void* buf) {
    if (g_exception_frame_count < MAX_EXCEPTION_FRAMES) {
        int idx = g_exception_frame_count;
        g_exception_frames[idx].buf = buf;
        g_exception_frames[idx].active = 1;
        g_exception_frames[idx].with_depth = g_with_manager_count;
        g_exception_frame_count++;
        return;
    }
    fprintf(stderr, "RuntimeError: too many nested try blocks\n");
    exit(1);
}

/* Exit the current try block */
void cool_exit_try(void) {
    if (g_exception_frame_count > 0) {
        g_exception_frame_count--;
        g_exception_frames[g_exception_frame_count].active = 0;
    }
}

/* Raise an exception - transfers control to the nearest try frame */
void cool_raise(CoolVal exc) {
    g_current_exception = exc;
    for (int i = g_exception_frame_count - 1; i >= 0; i--) {
        if (g_exception_frames[i].active) {
            cool_unwind_withs_to(g_exception_frames[i].with_depth);
            g_exception_frames[i].active = 0;
            longjmp(*(jmp_buf*)g_exception_frames[i].buf, 1);
        }
    }
    cool_unwind_withs_to(0);
    /* No try frame found - print and exit */
    char* msg = cool_to_str(exc);
    fprintf(stderr, "Unhandled exception: %s\n", msg);
    exit(1);
}

/* Get the current exception value */
CoolVal cool_get_exception(void) {
    return g_current_exception;
}

static double cool_now_monotonic_secs(void) {
#if defined(CLOCK_MONOTONIC)
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1000000000.0;
#else
    return (double)time(NULL);
#endif
}

static void cool_read_pipe_into_sb(int fd, CoolStrBuf* sb, int* open_flag) {
    char buf[4096];
    while (1) {
        ssize_t n = read(fd, buf, sizeof(buf) - 1);
        if (n > 0) {
            buf[n] = '\0';
            sb_push_str(sb, buf);
            continue;
        }
        if (n == 0) {
            close(fd);
            *open_flag = 0;
            return;
        }
        if (errno == EINTR) continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK) return;
        close(fd);
        *open_flag = 0;
        return;
    }
}

static CoolSubprocessResult cool_subprocess_run_shell(const char* cmd, int has_timeout, double timeout_secs) {
    CoolSubprocessResult result;
    result.has_code = 0;
    result.code = 0;
    result.timed_out = 0;
    result.stdout_data = strdup("");
    result.stderr_data = strdup("");

    int stdout_pipe[2];
    int stderr_pipe[2];
    if (pipe(stdout_pipe) != 0 || pipe(stderr_pipe) != 0) {
        fprintf(stderr, "RuntimeError: subprocess pipe failed\n");
        exit(1);
    }

    pid_t pid = fork();
    if (pid < 0) {
        fprintf(stderr, "RuntimeError: subprocess fork failed\n");
        exit(1);
    }

    if (pid == 0) {
        dup2(stdout_pipe[1], STDOUT_FILENO);
        dup2(stderr_pipe[1], STDERR_FILENO);
        close(stdout_pipe[0]);
        close(stdout_pipe[1]);
        close(stderr_pipe[0]);
        close(stderr_pipe[1]);
        execl("/bin/sh", "sh", "-c", cmd, (char*)NULL);
        _exit(127);
    }

    close(stdout_pipe[1]);
    close(stderr_pipe[1]);
    fcntl(stdout_pipe[0], F_SETFL, fcntl(stdout_pipe[0], F_GETFL, 0) | O_NONBLOCK);
    fcntl(stderr_pipe[0], F_SETFL, fcntl(stderr_pipe[0], F_GETFL, 0) | O_NONBLOCK);

    CoolStrBuf out_sb;
    CoolStrBuf err_sb;
    sb_init(&out_sb);
    sb_init(&err_sb);
    int stdout_open = 1;
    int stderr_open = 1;
    int child_done = 0;
    int status = 0;
    double start = cool_now_monotonic_secs();

    while (!child_done || stdout_open || stderr_open) {
        if (!child_done) {
            pid_t waited = waitpid(pid, &status, WNOHANG);
            if (waited == pid) {
                child_done = 1;
            } else if (waited < 0 && errno != EINTR) {
                break;
            }
        }

        if (!child_done && has_timeout && (cool_now_monotonic_secs() - start) >= timeout_secs) {
            result.timed_out = 1;
            kill(pid, SIGKILL);
            waitpid(pid, &status, 0);
            child_done = 1;
        }

        fd_set readfds;
        FD_ZERO(&readfds);
        int maxfd = -1;
        if (stdout_open) {
            FD_SET(stdout_pipe[0], &readfds);
            if (stdout_pipe[0] > maxfd) maxfd = stdout_pipe[0];
        }
        if (stderr_open) {
            FD_SET(stderr_pipe[0], &readfds);
            if (stderr_pipe[0] > maxfd) maxfd = stderr_pipe[0];
        }

        if (maxfd >= 0) {
            struct timeval tv;
            tv.tv_sec = 0;
            tv.tv_usec = child_done ? 0 : 50000;
            int ready = select(maxfd + 1, &readfds, NULL, NULL, &tv);
            if (ready > 0) {
                if (stdout_open && FD_ISSET(stdout_pipe[0], &readfds)) {
                    cool_read_pipe_into_sb(stdout_pipe[0], &out_sb, &stdout_open);
                }
                if (stderr_open && FD_ISSET(stderr_pipe[0], &readfds)) {
                    cool_read_pipe_into_sb(stderr_pipe[0], &err_sb, &stderr_open);
                }
            } else if (ready < 0 && errno != EINTR) {
                break;
            }
        }

        if (child_done) {
            if (stdout_open) cool_read_pipe_into_sb(stdout_pipe[0], &out_sb, &stdout_open);
            if (stderr_open) cool_read_pipe_into_sb(stderr_pipe[0], &err_sb, &stderr_open);
        }
    }

    result.stdout_data = out_sb.data;
    result.stderr_data = err_sb.data;
    if (WIFEXITED(status)) {
        result.has_code = 1;
        result.code = WEXITSTATUS(status);
    }
    return result;
}

static CoolVal cool_subprocess_result_dict(CoolSubprocessResult result) {
    CoolVal dict = cool_dict_new();
    cool_setindex(dict, cv_str("code"), result.has_code ? cv_int(result.code) : cv_nil());
    cool_setindex(dict, cv_str("stdout"), cv_str(result.stdout_data));
    cool_setindex(dict, cv_str("stderr"), cv_str(result.stderr_data));
    cool_setindex(dict, cv_str("timed_out"), cv_bool(result.timed_out));
    cool_setindex(
        dict,
        cv_str("ok"),
        cv_bool(!result.timed_out && result.has_code && result.code == 0)
    );
    return dict;
}

/* ── Module registry for import support ────────────────────────────────── */

#define MAX_MODULES 32

typedef struct {
    const char* name;
    CoolVal dict;
} ModuleEntry;

static ModuleEntry g_modules[MAX_MODULES];
static int g_module_count = 0;
static int g_modules_initialized = 0;

void cool_init_modules(void) {
    if (g_modules_initialized) return;
    g_modules_initialized = 1;
    
    /* math module */
    CoolDict* math_d = (CoolDict*)malloc(sizeof(CoolDict));
    math_d->tag = TAG_DICT; math_d->len = 0; math_d->cap = 16;
    math_d->keys = (CoolVal*)calloc(16, sizeof(CoolVal));
    math_d->vals = (CoolVal*)calloc(16, sizeof(CoolVal));
    CoolVal math_v; math_v.tag = TAG_DICT; math_v.payload = (int64_t)(intptr_t)math_d;
    g_modules[g_module_count].name = "math";
    g_modules[g_module_count++].dict = math_v;
    
    /* os module */
    CoolDict* os_d = (CoolDict*)malloc(sizeof(CoolDict));
    os_d->tag = TAG_DICT; os_d->len = 0; os_d->cap = 16;
    os_d->keys = (CoolVal*)calloc(16, sizeof(CoolVal));
    os_d->vals = (CoolVal*)calloc(16, sizeof(CoolVal));
    CoolVal os_v; os_v.tag = TAG_DICT; os_v.payload = (int64_t)(intptr_t)os_d;
    g_modules[g_module_count].name = "os";
    g_modules[g_module_count++].dict = os_v;
}

CoolVal cool_get_module(const char* name) {
    cool_init_modules();
    for (int i = 0; i < g_module_count; i++) {
        if (strcmp(g_modules[i].name, name) == 0) {
            return g_modules[i].dict;
        }
    }
    return cv_nil();
}

int32_t cool_module_exists(const char* name) {
    cool_init_modules();
    for (int i = 0; i < g_module_count; i++) {
        if (strcmp(g_modules[i].name, name) == 0) {
            return 1;
        }
    }
    return 0;
}

/* ── Function pointer call helper ─────────────────────────────────────── */

typedef CoolVal (*CoolFnPtr)(void);

CoolVal cool_call_fn_ptr(int64_t fn_ptr, int32_t nargs, ...) {
    va_list ap;
    va_start(ap, nargs);
    CoolVal argv[8];
    for (int32_t i = 0; i < nargs && i < 8; i++) {
        argv[i] = va_arg(ap, CoolVal);
    }
    va_end(ap);
    return call_cool_fn_ptr(fn_ptr, nargs, argv);
}
"#;

// ── Runtime function table ────────────────────────────────────────────────────

struct RuntimeFns<'ctx> {
    #[allow(dead_code)]
    cv_nil: FunctionValue<'ctx>,
    cv_int: FunctionValue<'ctx>,
    cv_float: FunctionValue<'ctx>,
    cv_bool: FunctionValue<'ctx>,
    cv_str: FunctionValue<'ctx>,
    cool_to_str: FunctionValue<'ctx>,
    cool_truthy: FunctionValue<'ctx>,
    cool_add: FunctionValue<'ctx>,
    cool_sub: FunctionValue<'ctx>,
    cool_mul: FunctionValue<'ctx>,
    cool_div: FunctionValue<'ctx>,
    cool_mod: FunctionValue<'ctx>,
    cool_pow: FunctionValue<'ctx>,
    cool_floordiv: FunctionValue<'ctx>,
    cool_neg: FunctionValue<'ctx>,
    cool_eq: FunctionValue<'ctx>,
    cool_neq: FunctionValue<'ctx>,
    cool_lt: FunctionValue<'ctx>,
    cool_lteq: FunctionValue<'ctx>,
    cool_gt: FunctionValue<'ctx>,
    cool_gteq: FunctionValue<'ctx>,
    cool_not: FunctionValue<'ctx>,
    cool_bitand: FunctionValue<'ctx>,
    cool_bitor: FunctionValue<'ctx>,
    cool_bitxor: FunctionValue<'ctx>,
    cool_bitnot: FunctionValue<'ctx>,
    cool_lshift: FunctionValue<'ctx>,
    cool_rshift: FunctionValue<'ctx>,
    /// void cool_print(int32_t n, ...)   — variadic
    cool_print: FunctionValue<'ctx>,
    /// void abort(void)
    abort_fn: FunctionValue<'ctx>,
    // raw memory
    cool_malloc: FunctionValue<'ctx>,
    cool_free: FunctionValue<'ctx>,
    cool_read_byte: FunctionValue<'ctx>,
    cool_write_byte: FunctionValue<'ctx>,
    cool_read_i64: FunctionValue<'ctx>,
    cool_write_i64: FunctionValue<'ctx>,
    cool_read_f64: FunctionValue<'ctx>,
    cool_write_f64: FunctionValue<'ctx>,
    cool_read_str: FunctionValue<'ctx>,
    cool_write_str: FunctionValue<'ctx>,
    // list/tuple operations
    cool_list_make: FunctionValue<'ctx>,
    cool_tuple_make: FunctionValue<'ctx>,
    cool_list_len: FunctionValue<'ctx>,
    cool_list_get: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_list_set: FunctionValue<'ctx>,
    cool_list_push: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_list_pop: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_list_concat: FunctionValue<'ctx>,
    // range
    cool_range: FunctionValue<'ctx>,
    // stdlib
    cool_len: FunctionValue<'ctx>,
    cool_type: FunctionValue<'ctx>,
    // class operations
    cool_class_new: FunctionValue<'ctx>,
    cool_object_new: FunctionValue<'ctx>,
    cool_get_attr: FunctionValue<'ctx>,
    cool_set_attr: FunctionValue<'ctx>,
    cool_call_method_vararg: FunctionValue<'ctx>,
    cool_get_arg: FunctionValue<'ctx>,
    cool_set_global_arg: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_is_instance: FunctionValue<'ctx>,
    cool_exception_matches: FunctionValue<'ctx>,
    cool_contains: FunctionValue<'ctx>,
    // dict operations
    cool_dict_new: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_dict_len: FunctionValue<'ctx>,
    cool_index: FunctionValue<'ctx>,
    cool_slice: FunctionValue<'ctx>,
    cool_setindex: FunctionValue<'ctx>,
    cool_file_open: FunctionValue<'ctx>,
    cool_abs: FunctionValue<'ctx>,
    cool_to_int: FunctionValue<'ctx>,
    cool_to_float_val: FunctionValue<'ctx>,
    cool_to_bool_val: FunctionValue<'ctx>,
    cool_noncallable: FunctionValue<'ctx>,
    cool_is_ffi_func: FunctionValue<'ctx>,
    cool_ffi_call: FunctionValue<'ctx>,
    cool_round: FunctionValue<'ctx>,
    cool_sorted: FunctionValue<'ctx>,
    cool_sum: FunctionValue<'ctx>,
    // closure operations
    #[allow(dead_code)]
    cool_closure_new: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_closure_get_fn_ptr: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_closure_get_capture: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_is_closure: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_set_closure_capture: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_get_closure_capture: FunctionValue<'ctx>,
    // exception handling
    #[allow(dead_code)]
    cool_enter_try: FunctionValue<'ctx>,
    #[allow(dead_code)]
    setjmp_fn: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_exit_try: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_raise: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_get_exception: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_register_class_parent: FunctionValue<'ctx>,
    cool_push_with: FunctionValue<'ctx>,
    cool_pop_with: FunctionValue<'ctx>,
    // module/import
    #[allow(dead_code)]
    cool_get_module: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_module_exists: FunctionValue<'ctx>,
    cool_module_get_attr: FunctionValue<'ctx>,
    cool_module_call: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_call_fn_ptr: FunctionValue<'ctx>,
}

// ── Compiler struct ───────────────────────────────────────────────────────────

struct Compiler<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    /// %CoolVal = type { i32, i64 }
    cv_type: StructType<'ctx>,
    rt: RuntimeFns<'ctx>,
    /// Current function's local variables (name → alloca pointer).
    locals: HashMap<String, PointerValue<'ctx>>,
    /// Top-level user-defined functions (name → FunctionValue).
    functions: HashMap<String, FunctionValue<'ctx>>,
    /// Top-level user-defined function signatures.
    function_params: HashMap<String, Vec<crate::ast::Param>>,
    /// Top-level user-defined classes (name → ClassInfo).
    classes: HashMap<String, ClassInfo<'ctx>>,
    str_counter: usize,
    /// (continue_target, break_target) for each enclosing loop.
    loop_stack: Vec<LoopFrame<'ctx>>,
    /// The function currently being compiled (Some(main_fn) at top level).
    current_fn: Option<FunctionValue<'ctx>>,
    /// Captured variables for closures (var name → capture index).
    #[allow(dead_code)]
    captured_vars: HashMap<String, usize>,
    /// All nested function definitions (for closure support).
    nested_functions: Vec<(String, Vec<crate::ast::Param>, Vec<crate::ast::Stmt>)>,
    /// Class currently being compiled, if any.
    current_class: Option<String>,
    /// Names of imported built-in modules visible to the native backend.
    imported_modules: HashSet<String>,
    /// Imported user modules visible in the current scope (binding name → canonical path).
    imported_user_modules: HashMap<String, PathBuf>,
    /// Canonical path → compiled module helper metadata.
    compiled_modules: HashMap<PathBuf, ModuleInfo<'ctx>>,
    /// Module files currently being compiled (for circular import detection).
    compiling_modules: Vec<PathBuf>,
    /// Source directory used to resolve relative imports for the current compilation unit.
    current_source_dir: PathBuf,
    /// Prefix applied to generated LLVM symbols for the current compilation unit.
    symbol_prefix: String,
    /// Whether the current compilation unit accepts top-level `def` and `class` statements.
    allow_toplevel_defs: bool,
    /// Active cleanup scopes in the current function.
    cleanup_stack: Vec<CleanupEntry<'ctx>>,
    /// Active try contexts in the current function.
    try_stack: Vec<TryContext>,
}

/// Information about a compiled class
#[derive(Clone)]
struct ClassInfo<'ctx> {
    /// The class constructor function (returns CoolVal)
    constructor: FunctionValue<'ctx>,
    /// Method names and their function values
    #[allow(dead_code)]
    methods: HashMap<String, FunctionValue<'ctx>>,
    /// Method signatures, including the leading `self` parameter.
    method_params: HashMap<String, Vec<crate::ast::Param>>,
    /// Attribute default values (compiled)
    #[allow(dead_code)]
    attributes: Vec<(String, Expr)>,
    /// Optional parent class name.
    parent: Option<String>,
    /// Constructor parameter list, excluding the implicit `self`.
    constructor_params: Vec<crate::ast::Param>,
}

#[derive(Clone)]
struct ModuleInfo<'ctx> {
    init_fn: FunctionValue<'ctx>,
    exports: Vec<String>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    function_params: HashMap<String, Vec<crate::ast::Param>>,
    classes: HashMap<String, ClassInfo<'ctx>>,
}

#[derive(Clone, Copy)]
struct LoopFrame<'ctx> {
    continue_bb: BasicBlock<'ctx>,
    break_bb: BasicBlock<'ctx>,
    cleanup_depth: usize,
}

#[derive(Clone)]
enum CleanupEntry<'ctx> {
    With { manager_ptr: PointerValue<'ctx> },
    Finally { body: Vec<Stmt> },
}

#[derive(Clone, Copy)]
struct TryContext {
    cleanup_depth: usize,
    catches_exceptions: bool,
}

// ── Constructor & runtime declarations ───────────────────────────────────────

impl<'ctx> Compiler<'ctx> {
    fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("cool_program");
        let builder = context.create_builder();

        // %CoolVal = type { i32, i64 }
        let cv_type = context.opaque_struct_type("CoolVal");
        cv_type.set_body(&[context.i32_type().into(), context.i64_type().into()], false);

        let rt = Self::declare_runtime(context, &module, cv_type);

        Compiler {
            context,
            module,
            builder,
            cv_type,
            rt,
            locals: HashMap::new(),
            functions: HashMap::new(),
            function_params: HashMap::new(),
            classes: HashMap::new(),
            str_counter: 0,
            loop_stack: Vec::new(),
            current_fn: None,
            captured_vars: HashMap::new(),
            nested_functions: Vec::new(),
            current_class: None,
            imported_modules: HashSet::new(),
            imported_user_modules: HashMap::new(),
            compiled_modules: HashMap::new(),
            compiling_modules: Vec::new(),
            current_source_dir: PathBuf::from("."),
            symbol_prefix: String::new(),
            allow_toplevel_defs: false,
            cleanup_stack: Vec::new(),
            try_stack: Vec::new(),
        }
    }

    fn declare_runtime(context: &'ctx Context, module: &Module<'ctx>, cv_type: StructType<'ctx>) -> RuntimeFns<'ctx> {
        let i32t = context.i32_type();
        let i64t = context.i64_type();
        let f64t = context.f64_type();
        let voidt = context.void_type();
        let ptr_t = context.i8_type().ptr_type(AddressSpace::default());

        // BasicMetadataTypeEnum variants (all Copy)
        let cv: inkwell::types::BasicMetadataTypeEnum<'ctx> = cv_type.into();
        let i32m: inkwell::types::BasicMetadataTypeEnum<'ctx> = i32t.into();
        let i64m: inkwell::types::BasicMetadataTypeEnum<'ctx> = i64t.into();
        let f64m: inkwell::types::BasicMetadataTypeEnum<'ctx> = f64t.into();
        let ptrm: inkwell::types::BasicMetadataTypeEnum<'ctx> = ptr_t.into();

        macro_rules! decl {
            ($name:expr, $ty:expr) => {
                module.add_function($name, $ty, None)
            };
        }

        RuntimeFns {
            cv_nil: decl!("cv_nil", cv_type.fn_type(&[], false)),
            cv_int: decl!("cv_int", cv_type.fn_type(&[i64m], false)),
            cv_float: decl!("cv_float", cv_type.fn_type(&[f64m], false)),
            cv_bool: decl!("cv_bool", cv_type.fn_type(&[i32m], false)),
            cv_str: decl!("cv_str", cv_type.fn_type(&[ptrm], false)),
            cool_to_str: decl!("cool_to_str", ptr_t.fn_type(&[cv], false)),
            cool_truthy: decl!("cool_truthy", i32t.fn_type(&[cv], false)),
            cool_add: decl!("cool_add", cv_type.fn_type(&[cv, cv], false)),
            cool_sub: decl!("cool_sub", cv_type.fn_type(&[cv, cv], false)),
            cool_mul: decl!("cool_mul", cv_type.fn_type(&[cv, cv], false)),
            cool_div: decl!("cool_div", cv_type.fn_type(&[cv, cv], false)),
            cool_mod: decl!("cool_mod", cv_type.fn_type(&[cv, cv], false)),
            cool_pow: decl!("cool_pow", cv_type.fn_type(&[cv, cv], false)),
            cool_floordiv: decl!("cool_floordiv", cv_type.fn_type(&[cv, cv], false)),
            cool_neg: decl!("cool_neg", cv_type.fn_type(&[cv], false)),
            cool_eq: decl!("cool_eq", cv_type.fn_type(&[cv, cv], false)),
            cool_neq: decl!("cool_neq", cv_type.fn_type(&[cv, cv], false)),
            cool_lt: decl!("cool_lt", cv_type.fn_type(&[cv, cv], false)),
            cool_lteq: decl!("cool_lteq", cv_type.fn_type(&[cv, cv], false)),
            cool_gt: decl!("cool_gt", cv_type.fn_type(&[cv, cv], false)),
            cool_gteq: decl!("cool_gteq", cv_type.fn_type(&[cv, cv], false)),
            cool_not: decl!("cool_not", cv_type.fn_type(&[cv], false)),
            cool_bitand: decl!("cool_bitand", cv_type.fn_type(&[cv, cv], false)),
            cool_bitor: decl!("cool_bitor", cv_type.fn_type(&[cv, cv], false)),
            cool_bitxor: decl!("cool_bitxor", cv_type.fn_type(&[cv, cv], false)),
            cool_bitnot: decl!("cool_bitnot", cv_type.fn_type(&[cv], false)),
            cool_lshift: decl!("cool_lshift", cv_type.fn_type(&[cv, cv], false)),
            cool_rshift: decl!("cool_rshift", cv_type.fn_type(&[cv, cv], false)),
            // void cool_print(i32 n, ...)  — is_var_arg = true
            cool_print: decl!("cool_print", voidt.fn_type(&[i32m], true)),
            abort_fn: decl!("abort", voidt.fn_type(&[], false)),
            // raw memory — all take CoolVal(s) and return CoolVal
            cool_malloc: decl!("cool_malloc", cv_type.fn_type(&[cv], false)),
            cool_free: decl!("cool_free", cv_type.fn_type(&[cv], false)),
            cool_read_byte: decl!("cool_read_byte", cv_type.fn_type(&[cv], false)),
            cool_write_byte: decl!("cool_write_byte", cv_type.fn_type(&[cv, cv], false)),
            cool_read_i64: decl!("cool_read_i64", cv_type.fn_type(&[cv], false)),
            cool_write_i64: decl!("cool_write_i64", cv_type.fn_type(&[cv, cv], false)),
            cool_read_f64: decl!("cool_read_f64", cv_type.fn_type(&[cv], false)),
            cool_write_f64: decl!("cool_write_f64", cv_type.fn_type(&[cv, cv], false)),
            cool_read_str: decl!("cool_read_str", cv_type.fn_type(&[cv], false)),
            cool_write_str: decl!("cool_write_str", cv_type.fn_type(&[cv, cv], false)),
            // list operations
            cool_list_make: decl!("cool_list_make", cv_type.fn_type(&[cv], false)),
            cool_tuple_make: decl!("cool_tuple_make", cv_type.fn_type(&[cv], false)),
            cool_list_len: decl!("cool_list_len", cv_type.fn_type(&[cv], false)),
            cool_list_get: decl!("cool_list_get", cv_type.fn_type(&[cv, cv], false)),
            cool_list_set: decl!("cool_list_set", cv_type.fn_type(&[cv, cv, cv], false)),
            cool_list_push: decl!("cool_list_push", cv_type.fn_type(&[cv, cv], false)),
            cool_list_pop: decl!("cool_list_pop", cv_type.fn_type(&[cv], false)),
            cool_list_concat: decl!("cool_list_concat", cv_type.fn_type(&[cv, cv], false)),
            // range(start, stop, step)
            cool_range: decl!("cool_range", cv_type.fn_type(&[cv, cv, cv], false)),
            // len(obj)
            cool_len: decl!("cool_len", cv_type.fn_type(&[cv], false)),
            cool_type: decl!("cool_type", cv_type.fn_type(&[cv], false)),
            // class operations
            cool_class_new: decl!("cool_class_new", cv_type.fn_type(&[ptrm, cv, i64m, ptrm], false)),
            cool_object_new: decl!("cool_object_new", cv_type.fn_type(&[cv], false)),
            cool_get_attr: decl!("cool_get_attr", cv_type.fn_type(&[cv, ptrm], false)),
            cool_set_attr: decl!("cool_set_attr", cv_type.fn_type(&[cv, ptrm, cv], false)),
            cool_call_method_vararg: decl!("cool_call_method_vararg", cv_type.fn_type(&[cv, ptrm, i32m], true)),
            cool_get_arg: decl!("cool_get_arg", cv_type.fn_type(&[i32m], false)),
            cool_set_global_arg: decl!("cool_set_global_arg", voidt.fn_type(&[i32m, cv], false)),
            cool_is_instance: decl!("cool_is_instance", cv_type.fn_type(&[cv, ptrm], false)),
            cool_exception_matches: decl!("cool_exception_matches", i32t.fn_type(&[cv, ptrm], false)),
            cool_contains: decl!("cool_contains", cv_type.fn_type(&[cv, cv], false)),
            // dict operations
            cool_dict_new: decl!("cool_dict_new", cv_type.fn_type(&[], false)),
            cool_dict_len: decl!("cool_dict_len", cv_type.fn_type(&[cv], false)),
            cool_index: decl!("cool_index", cv_type.fn_type(&[cv, cv], false)),
            cool_slice: decl!("cool_slice", cv_type.fn_type(&[cv, cv, cv], false)),
            cool_setindex: decl!("cool_setindex", cv_type.fn_type(&[cv, cv, cv], false)),
            cool_file_open: decl!("cool_file_open", cv_type.fn_type(&[cv, cv], false)),
            cool_abs: decl!("cool_abs", cv_type.fn_type(&[cv], false)),
            cool_to_int: decl!("cool_to_int", cv_type.fn_type(&[cv], false)),
            cool_to_float_val: decl!("cool_to_float_val", cv_type.fn_type(&[cv], false)),
            cool_to_bool_val: decl!("cool_to_bool_val", cv_type.fn_type(&[cv], false)),
            cool_noncallable: decl!("cool_noncallable", cv_type.fn_type(&[cv], false)),
            cool_is_ffi_func: decl!("cool_is_ffi_func", i32t.fn_type(&[cv], false)),
            cool_ffi_call: decl!("cool_ffi_call", cv_type.fn_type(&[cv, i32m], true)),
            cool_round: decl!("cool_round", cv_type.fn_type(&[cv, cv], false)),
            cool_sorted: decl!("cool_sorted", cv_type.fn_type(&[cv], false)),
            cool_sum: decl!("cool_sum", cv_type.fn_type(&[cv], false)),
            // closure operations
            cool_closure_new: decl!("cool_closure_new", cv_type.fn_type(&[i64m, i64m, ptrm], false)),
            cool_closure_get_fn_ptr: decl!("cool_closure_get_fn_ptr", i64t.fn_type(&[cv], false)),
            cool_closure_get_capture: decl!("cool_closure_get_capture", cv_type.fn_type(&[cv, i64m], false)),
            cool_is_closure: decl!("cool_is_closure", i32t.fn_type(&[cv], false)),
            cool_set_closure_capture: decl!("cool_set_closure_capture", voidt.fn_type(&[i32m, cv], false)),
            cool_get_closure_capture: decl!("cool_get_closure_capture", cv_type.fn_type(&[i32m], false)),
            // exception handling
            cool_enter_try: decl!("cool_enter_try", voidt.fn_type(&[ptrm], false)),
            setjmp_fn: decl!("_setjmp", i32t.fn_type(&[ptrm], false)),
            cool_exit_try: decl!("cool_exit_try", voidt.fn_type(&[], false)),
            cool_raise: decl!("cool_raise", voidt.fn_type(&[cv], false)),
            cool_get_exception: decl!("cool_get_exception", cv_type.fn_type(&[], false)),
            cool_register_class_parent: decl!("cool_register_class_parent", voidt.fn_type(&[ptrm, ptrm], false)),
            cool_push_with: decl!("cool_push_with", voidt.fn_type(&[cv], false)),
            cool_pop_with: decl!("cool_pop_with", voidt.fn_type(&[], false)),
            // module/import
            cool_get_module: decl!("cool_get_module", cv_type.fn_type(&[ptrm], false)),
            cool_module_exists: decl!("cool_module_exists", i32t.fn_type(&[ptrm], false)),
            cool_module_get_attr: decl!("cool_module_get_attr", cv_type.fn_type(&[ptrm, ptrm], false)),
            cool_module_call: decl!("cool_module_call", cv_type.fn_type(&[ptrm, ptrm, i32m], true)),
            cool_call_fn_ptr: decl!("cool_call_fn_ptr", cv_type.fn_type(&[i64m, i32m], true)),
        }
    }

    // ── Small helpers ─────────────────────────────────────────────────────────

    fn current_block_terminated(&self) -> bool {
        self.builder
            .get_insert_block()
            .and_then(|bb| bb.get_terminator())
            .is_some()
    }

    fn is_entry_main(&self) -> bool {
        self.current_fn
            .map(|f| f.get_name().to_str().unwrap_or("") == "main")
            .unwrap_or(false)
    }

    fn mangle_global_name(&self, name: &str) -> String {
        if self.symbol_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}{}", self.symbol_prefix, name)
        }
    }

    fn declare_top_level_functions(&mut self, program: &Program) -> Result<(), String> {
        for stmt in program {
            if let Stmt::FnDef { name, params, .. } = stmt {
                if self.functions.contains_key(name) {
                    continue;
                }
                if params.iter().any(|p| p.is_vararg || p.is_kwarg) {
                    return Err(format!(
                        "function '{name}': *args / **kwargs are not supported in LLVM backend"
                    ));
                }
                let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'_>> =
                    params.iter().map(|_| self.cv_type.into()).collect();
                let fn_type = self.cv_type.fn_type(&param_types, false);
                let fn_name = self.mangle_global_name(name);
                let fn_val = self.module.add_function(&fn_name, fn_type, None);
                self.functions.insert(name.clone(), fn_val);
                self.function_params.insert(name.clone(), params.clone());
            }
        }
        Ok(())
    }

    fn resolve_import_file_path(&self, path: &str) -> Result<PathBuf, String> {
        let full_path = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.current_source_dir.join(path)
        };
        if full_path.exists() {
            Ok(full_path.canonicalize().unwrap_or(full_path))
        } else {
            Err(format!("import error: file not found: {}", full_path.display()))
        }
    }

    fn resolve_import_module_path(&self, name: &str) -> Option<PathBuf> {
        let file_path = name.replace('.', "/");
        let candidates = [
            self.current_source_dir.join(format!("{}.cool", file_path)),
            self.current_source_dir.join(&file_path).join("__init__.cool"),
            PathBuf::from(format!("lib/{}.cool", file_path)),
        ];
        candidates
            .into_iter()
            .find(|path| path.exists())
            .map(|path| path.canonicalize().unwrap_or(path))
    }

    fn fresh_name(&mut self) -> String {
        let n = self.str_counter;
        self.str_counter += 1;
        format!("s{n}")
    }

    fn build_entry_alloca(&self, name: &str) -> PointerValue<'ctx> {
        let fn_val = self.current_fn.expect("alloca requires active function");
        let entry = fn_val
            .get_first_basic_block()
            .expect("function should have an entry block");
        let builder = self.context.create_builder();
        if let Some(first) = entry.get_first_instruction() {
            builder.position_before(&first);
        } else {
            builder.position_at_end(entry);
        }
        builder.build_alloca(self.cv_type, name).unwrap()
    }

    fn build_entry_jmp_buf_alloca(&self, name: &str) -> PointerValue<'ctx> {
        let fn_val = self.current_fn.expect("jmp_buf alloca requires active function");
        let entry = fn_val
            .get_first_basic_block()
            .expect("function should have an entry block");
        let builder = self.context.create_builder();
        if let Some(first) = entry.get_first_instruction() {
            builder.position_before(&first);
        } else {
            builder.position_at_end(entry);
        }
        let buf_ty = self.context.i64_type().array_type(64);
        builder.build_alloca(buf_ty, name).unwrap()
    }

    fn current_cleanup_depth(&self) -> usize {
        self.cleanup_stack.len()
    }

    fn call_method_named(
        &mut self,
        obj: StructValue<'ctx>,
        method_name: &str,
        args: &[StructValue<'ctx>],
        name: &str,
    ) -> StructValue<'ctx> {
        let method_label = format!("method_{}", method_name);
        let global_name = format!("{}_{}", name, self.fresh_name());
        let method_ptr = self
            .builder
            .build_global_string_ptr(&method_label, &global_name)
            .unwrap();
        let nargs_i32 = self.context.i32_type().const_int(args.len() as u64, false);
        let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> =
            vec![obj.into(), method_ptr.as_pointer_value().into(), nargs_i32.into()];
        for arg in args {
            call_args.push((*arg).into());
        }
        self.builder
            .build_call(self.rt.cool_call_method_vararg, &call_args, name)
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    fn emit_with_exit_call(&mut self, manager_ptr: PointerValue<'ctx>) {
        let manager = self
            .builder
            .build_load(self.cv_type, manager_ptr, "with_manager")
            .unwrap()
            .into_struct_value();
        let nil0 = self.build_nil();
        let nil1 = self.build_nil();
        let nil2 = self.build_nil();
        let _ = self.call_method_named(manager, "__exit__", &[nil0, nil1, nil2], "with_exit");
        self.builder.build_call(self.rt.cool_pop_with, &[], "with_pop").unwrap();
    }

    fn emit_cleanup_entry(&mut self, entry_index: usize, entry: CleanupEntry<'ctx>) -> Result<(), String> {
        match entry {
            CleanupEntry::With { manager_ptr } => {
                self.emit_with_exit_call(manager_ptr);
                Ok(())
            }
            CleanupEntry::Finally { body } => {
                let saved_cleanup_stack = self.cleanup_stack.clone();
                let saved_try_stack = self.try_stack.clone();
                self.cleanup_stack.truncate(entry_index);
                self.try_stack.retain(|ctx| ctx.cleanup_depth < entry_index);
                let result = self.compile_stmts(&body);
                self.cleanup_stack = saved_cleanup_stack;
                self.try_stack = saved_try_stack;
                result
            }
        }
    }

    fn emit_try_exit_from_cleanup_depth(&mut self, depth: usize) {
        for ctx in self.try_stack.iter().rev() {
            if ctx.catches_exceptions && ctx.cleanup_depth >= depth {
                self.builder
                    .build_call(self.rt.cool_exit_try, &[], "exit_try_for_control_flow")
                    .unwrap();
            }
        }
    }

    fn emit_cleanup_from_depth(&mut self, depth: usize) -> Result<(), String> {
        let entries: Vec<(usize, CleanupEntry<'ctx>)> = self.cleanup_stack[depth..]
            .iter()
            .cloned()
            .enumerate()
            .map(|(offset, entry)| (depth + offset, entry))
            .collect();
        for (entry_index, entry) in entries.into_iter().rev() {
            self.emit_cleanup_entry(entry_index, entry)?;
            if self.current_block_terminated() {
                break;
            }
        }
        Ok(())
    }

    fn current_raise_cleanup_depth(&self) -> Option<usize> {
        match self.try_stack.last() {
            Some(ctx) if ctx.catches_exceptions => None,
            Some(ctx) => Some(ctx.cleanup_depth),
            None => Some(0),
        }
    }

    // ── CoolVal constructors ──────────────────────────────────────────────────

    fn build_nil(&mut self) -> StructValue<'ctx> {
        self.builder
            .build_call(self.rt.cv_nil, &[], "nil")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    fn build_int(&mut self, n: i64) -> StructValue<'ctx> {
        let v = self.context.i64_type().const_int(n as u64, true);
        self.builder
            .build_call(self.rt.cv_int, &[v.into()], "int")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    fn build_float(&mut self, f: f64) -> StructValue<'ctx> {
        let v = self.context.f64_type().const_float(f);
        self.builder
            .build_call(self.rt.cv_float, &[v.into()], "float")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    fn build_bool(&mut self, b: bool) -> StructValue<'ctx> {
        let v = self.context.i32_type().const_int(u64::from(b), false);
        self.builder
            .build_call(self.rt.cv_bool, &[v.into()], "bool")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    fn build_str(&mut self, s: &str) -> StructValue<'ctx> {
        let name = self.fresh_name();
        let gbl = self.builder.build_global_string_ptr(s, &name).unwrap();
        let ptr = gbl.as_pointer_value();
        self.builder
            .build_call(self.rt.cv_str, &[ptr.into()], "str")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    // Returns an i32 (0 or 1).
    fn build_truthy(&mut self, v: StructValue<'ctx>) -> IntValue<'ctx> {
        self.builder
            .build_call(self.rt.cool_truthy, &[v.into()], "truthy")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value()
    }

    // Returns an i1 suitable for build_conditional_branch.
    fn truthy_i1(&mut self, v: StructValue<'ctx>) -> IntValue<'ctx> {
        let t = self.build_truthy(v);
        let zero = self.context.i32_type().const_int(0, false);
        self.builder
            .build_int_compare(IntPredicate::NE, t, zero, "cond")
            .unwrap()
    }

    // Call a binary-op runtime function.
    fn call_binop_fn(
        &mut self,
        fn_val: FunctionValue<'ctx>,
        a: StructValue<'ctx>,
        b: StructValue<'ctx>,
        name: &str,
    ) -> StructValue<'ctx> {
        self.builder
            .build_call(fn_val, &[a.into(), b.into()], name)
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    // Call a ternary runtime function (three CoolVal args, returns CoolVal).
    fn call_triop_fn(
        &mut self,
        fn_val: FunctionValue<'ctx>,
        a: StructValue<'ctx>,
        b: StructValue<'ctx>,
        c: StructValue<'ctx>,
        name: &str,
    ) -> StructValue<'ctx> {
        self.builder
            .build_call(fn_val, &[a.into(), b.into(), c.into()], name)
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    // Call a unary-op runtime function.
    fn call_unop_fn(&mut self, fn_val: FunctionValue<'ctx>, a: StructValue<'ctx>, name: &str) -> StructValue<'ctx> {
        self.builder
            .build_call(fn_val, &[a.into()], name)
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    // ── apply_binop: map BinOp → runtime fn, no short-circuit ────────────────

    fn apply_binop(
        &mut self,
        op: &BinOp,
        a: StructValue<'ctx>,
        b: StructValue<'ctx>,
    ) -> Result<StructValue<'ctx>, String> {
        let fn_val = match op {
            BinOp::Add => self.rt.cool_add,
            BinOp::Sub => self.rt.cool_sub,
            BinOp::Mul => self.rt.cool_mul,
            BinOp::Div => self.rt.cool_div,
            BinOp::Mod => self.rt.cool_mod,
            BinOp::Pow => self.rt.cool_pow,
            BinOp::FloorDiv => self.rt.cool_floordiv,
            BinOp::Eq => self.rt.cool_eq,
            BinOp::NotEq => self.rt.cool_neq,
            BinOp::Lt => self.rt.cool_lt,
            BinOp::LtEq => self.rt.cool_lteq,
            BinOp::Gt => self.rt.cool_gt,
            BinOp::GtEq => self.rt.cool_gteq,
            BinOp::BitAnd => self.rt.cool_bitand,
            BinOp::BitOr => self.rt.cool_bitor,
            BinOp::BitXor => self.rt.cool_bitxor,
            BinOp::LShift => self.rt.cool_lshift,
            BinOp::RShift => self.rt.cool_rshift,
            BinOp::And | BinOp::Or => {
                return Err("and/or cannot be used in augmented assignment".into());
            }
            BinOp::In | BinOp::NotIn => {
                return Err("'in'/'not in' not supported in augmented assignment".into());
            }
        };
        Ok(self.call_binop_fn(fn_val, a, b, "binop"))
    }

    // ── Statement compiler ────────────────────────────────────────────────────

    fn compile_stmts(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for stmt in stmts {
            if self.current_block_terminated() {
                break; // dead code — skip the rest
            }
            self.compile_stmt(stmt)?;
        }
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            // ── no-ops ────────────────────────────────────────────────────────
            Stmt::SetLine(_) | Stmt::Pass => {}

            // ── expression statement ──────────────────────────────────────────
            Stmt::Expr(expr) => {
                self.compile_expr(expr)?; // result discarded
            }

            // ── variable assignment ───────────────────────────────────────────
            Stmt::Assign { name, value } => {
                let val = self.compile_expr(value)?;
                let ptr = if let Some(&p) = self.locals.get(name) {
                    p
                } else {
                    let p = self.build_entry_alloca(name);
                    self.locals.insert(name.clone(), p);
                    p
                };
                self.builder.build_store(ptr, val).unwrap();
            }

            // ── augmented assignment  (x += expr, etc.) ───────────────────────
            Stmt::AugAssign { name, op, value } => {
                let ptr = self
                    .locals
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("undefined variable '{name}'"))?;
                let cur = self
                    .builder
                    .build_load(self.cv_type, ptr, "aug_load")
                    .unwrap()
                    .into_struct_value();
                let rhs = self.compile_expr(value)?;
                let result = self.apply_binop(op, cur, rhs)?;
                self.builder.build_store(ptr, result).unwrap();
            }

            // ── attribute assignment: obj.attr = value ────────────────────────
            Stmt::SetAttr { object, name, value } => {
                let obj_val = self.compile_expr(object)?;
                let val = self.compile_expr(value)?;
                let attr_name_ptr = self
                    .builder
                    .build_global_string_ptr(name, &format!("attr_{}", name))
                    .unwrap();
                self.builder
                    .build_call(
                        self.rt.cool_set_attr,
                        &[obj_val.into(), attr_name_ptr.as_pointer_value().into(), val.into()],
                        "set_attr",
                    )
                    .unwrap();
            }

            // ── return ────────────────────────────────────────────────────────
            Stmt::Return(opt_expr) => {
                if self.is_entry_main() {
                    // top-level return → exit normally
                    if let Some(e) = opt_expr {
                        self.compile_expr(e)?; // side-effects only
                    }
                    self.emit_try_exit_from_cleanup_depth(0);
                    self.emit_cleanup_from_depth(0)?;
                    let zero = self.context.i32_type().const_int(0, false);
                    self.builder.build_return(Some(&zero)).unwrap();
                } else {
                    let val = match opt_expr {
                        Some(e) => self.compile_expr(e)?,
                        None => self.build_nil(),
                    };
                    self.emit_try_exit_from_cleanup_depth(0);
                    self.emit_cleanup_from_depth(0)?;
                    self.builder.build_return(Some(&val)).unwrap();
                }
            }

            // ── break / continue ─────────────────────────────────────────────
            Stmt::Break => {
                let loop_frame = *self.loop_stack.last().ok_or("'break' used outside loop")?;
                self.emit_try_exit_from_cleanup_depth(loop_frame.cleanup_depth);
                self.emit_cleanup_from_depth(loop_frame.cleanup_depth)?;
                self.builder.build_unconditional_branch(loop_frame.break_bb).unwrap();
            }
            Stmt::Continue => {
                let loop_frame = *self.loop_stack.last().ok_or("'continue' used outside loop")?;
                self.emit_try_exit_from_cleanup_depth(loop_frame.cleanup_depth);
                self.emit_cleanup_from_depth(loop_frame.cleanup_depth)?;
                self.builder.build_unconditional_branch(loop_frame.continue_bb).unwrap();
            }

            // ── if / elif / else ─────────────────────────────────────────────
            Stmt::If {
                condition,
                then_body,
                elif_clauses,
                else_body,
            } => {
                self.compile_if(condition, then_body, elif_clauses, else_body)?;
            }

            // ── while ────────────────────────────────────────────────────────
            Stmt::While { condition, body } => {
                self.compile_while(condition, body)?;
            }

            // ── for var in iterable ────────────────────────────────────────────────
            Stmt::For { var, iter, body } => {
                self.compile_for(var, iter, body)?;
            }

            // ── function definition ───────────────────────────────────────────
            Stmt::FnDef { name, params, body } => {
                self.compile_fndef(name, params, body)?;
            }

            // ── class definition ────────────────────────────────────────────
            Stmt::Class { name, parent, body } => {
                self.compile_class(name, parent.as_deref(), body)?;
            }

            // ── with expr as name ───────────────────────────────────────────
            Stmt::With { expr, as_name, body } => {
                self.compile_with(expr, as_name.as_deref(), body)?;
            }

            // ── assert ────────────────────────────────────────────────────────
            Stmt::Assert { condition, message } => {
                self.compile_assert(condition, message.as_ref())?;
            }

            // ── tuple unpack: a, b, c = expr ─────────────────────────────────
            Stmt::Unpack { names, value } => {
                let seq = self.compile_expr(value)?;
                let seq_ptr = self.builder.build_alloca(self.cv_type, "unpack_seq").unwrap();
                self.builder.build_store(seq_ptr, seq).unwrap();
                for (i, name) in names.iter().enumerate() {
                    let idx_val = self.build_int(i as i64);
                    let seq_cur = self
                        .builder
                        .build_load(self.cv_type, seq_ptr, "unpack_seq")
                        .unwrap()
                        .into_struct_value();
                    let elem = self.call_binop_fn(self.rt.cool_index, seq_cur, idx_val, "unpack_elem");
                    let ptr = if let Some(&p) = self.locals.get(name) {
                        p
                    } else {
                        let p = self.build_entry_alloca(name);
                        self.locals.insert(name.clone(), p);
                        p
                    };
                    self.builder.build_store(ptr, elem).unwrap();
                }
            }

            // ── item assignment: obj[idx] = value ────────────────────────────
            Stmt::SetItem { object, index, value } => {
                let obj_val = self.compile_expr(object)?;
                let idx_val = self.compile_expr(index)?;
                let val = self.compile_expr(value)?;
                self.call_triop_fn(self.rt.cool_setindex, obj_val, idx_val, val, "setindex");
            }

            // ── try / except / else / finally ────────────────────────────────
            Stmt::Try {
                body,
                handlers,
                else_body,
                finally_body,
            } => {
                self.compile_try(body, handlers, else_body.as_deref(), finally_body.as_deref())?;
            }

            // ── raise ────────────────────────────────────────────────────────
            Stmt::Raise(opt_expr) => {
                self.compile_raise(opt_expr.as_ref())?;
            }

            // ── import file.cool ─────────────────────────────────────────────
            Stmt::Import(path) => {
                self.compile_import(path)?;
            }

            // ── import module_name ───────────────────────────────────────────
            Stmt::ImportModule(name) => {
                self.compile_import_module(name)?;
            }

            other => {
                return Err(format!("unsupported statement in LLVM backend: {other:?}"));
            }
        }
        Ok(())
    }

    // ── if / elif / else ──────────────────────────────────────────────────────

    fn compile_if(
        &mut self,
        condition: &Expr,
        then_body: &[Stmt],
        elif_clauses: &[(Expr, Vec<Stmt>)],
        else_body: &Option<Vec<Stmt>>,
    ) -> Result<(), String> {
        let fn_val = self.current_fn.unwrap();
        let merge_bb = self.context.append_basic_block(fn_val, "if_end");

        // ── main if branch ──
        let cond_cv = self.compile_expr(condition)?;
        let i1 = self.truthy_i1(cond_cv);
        let then_bb = self.context.append_basic_block(fn_val, "if_then");
        let else_entry = self.context.append_basic_block(fn_val, "if_else");
        self.builder.build_conditional_branch(i1, then_bb, else_entry).unwrap();

        // ── then body ──
        self.builder.position_at_end(then_bb);
        self.compile_stmts(then_body)?;
        if !self.current_block_terminated() {
            self.builder.build_unconditional_branch(merge_bb).unwrap();
        }

        // ── elif / else chain ──
        let mut cur_else = else_entry;
        for (elif_cond, elif_body) in elif_clauses {
            self.builder.position_at_end(cur_else);
            let cv = self.compile_expr(elif_cond)?;
            let i1 = self.truthy_i1(cv);
            let elif_then = self.context.append_basic_block(fn_val, "elif_then");
            let elif_else = self.context.append_basic_block(fn_val, "elif_else");
            self.builder.build_conditional_branch(i1, elif_then, elif_else).unwrap();

            self.builder.position_at_end(elif_then);
            self.compile_stmts(elif_body)?;
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }
            cur_else = elif_else;
        }

        // ── final else ──
        self.builder.position_at_end(cur_else);
        if let Some(stmts) = else_body {
            self.compile_stmts(stmts)?;
        }
        if !self.current_block_terminated() {
            self.builder.build_unconditional_branch(merge_bb).unwrap();
        }

        self.builder.position_at_end(merge_bb);
        Ok(())
    }

    // ── while ─────────────────────────────────────────────────────────────────

    fn compile_while(&mut self, condition: &Expr, body: &[Stmt]) -> Result<(), String> {
        let fn_val = self.current_fn.unwrap();
        let cond_bb = self.context.append_basic_block(fn_val, "while_cond");
        let body_bb = self.context.append_basic_block(fn_val, "while_body");
        let after_bb = self.context.append_basic_block(fn_val, "while_after");

        // Fall into condition check
        self.builder.build_unconditional_branch(cond_bb).unwrap();

        // Condition
        self.builder.position_at_end(cond_bb);
        let cond_cv = self.compile_expr(condition)?;
        let i1 = self.truthy_i1(cond_cv);
        self.builder.build_conditional_branch(i1, body_bb, after_bb).unwrap();

        // Body — push (continue→cond_bb, break→after_bb)
        self.loop_stack.push(LoopFrame {
            continue_bb: cond_bb,
            break_bb: after_bb,
            cleanup_depth: self.current_cleanup_depth(),
        });
        self.builder.position_at_end(body_bb);
        self.compile_stmts(body)?;
        if !self.current_block_terminated() {
            self.builder.build_unconditional_branch(cond_bb).unwrap();
        }
        self.loop_stack.pop();

        self.builder.position_at_end(after_bb);
        Ok(())
    }

    // ── for var in iterable ─────────────────────────────────────────────────
    fn compile_for(&mut self, var: &str, iter: &Expr, body: &[Stmt]) -> Result<(), String> {
        let fn_val = self.current_fn.unwrap();
        let cond_bb = self.context.append_basic_block(fn_val, "for_cond");
        let body_bb = self.context.append_basic_block(fn_val, "for_body");
        let step_bb = self.context.append_basic_block(fn_val, "for_step");
        let after_bb = self.context.append_basic_block(fn_val, "for_after");

        // Compile the iterable into an index variable
        let iter_val = self.compile_expr(iter)?;
        let idx_ptr = self.builder.build_alloca(self.cv_type, "for_idx").unwrap();
        let zero = self.build_int(0);
        self.builder.build_store(idx_ptr, zero).unwrap();

        // Allocate the loop variable
        let var_ptr = self.build_entry_alloca(var);
        self.locals.insert(var.to_string(), var_ptr);

        // Jump to condition check
        self.builder.build_unconditional_branch(cond_bb).unwrap();

        // Condition: check idx < len
        self.builder.position_at_end(cond_bb);
        let idx_cv = self
            .builder
            .build_load(self.cv_type, idx_ptr, "idx_load")
            .unwrap()
            .into_struct_value();
        let len_i64 = self.call_unop_fn(self.rt.cool_list_len, iter_val.clone(), "len");
        let cmp = self.call_binop_fn(self.rt.cool_lt, idx_cv, len_i64, "lt");
        let i1 = self.truthy_i1(cmp);
        self.builder.build_conditional_branch(i1, body_bb, after_bb).unwrap();

        // Body: get element at idx and execute body
        self.builder.position_at_end(body_bb);
        self.loop_stack.push(LoopFrame {
            continue_bb: step_bb,
            break_bb: after_bb,
            cleanup_depth: self.current_cleanup_depth(),
        });
        let body_idx = self
            .builder
            .build_load(self.cv_type, idx_ptr, "body_idx")
            .unwrap()
            .into_struct_value();
        let elem = self.call_binop_fn(self.rt.cool_list_get, iter_val.clone(), body_idx, "get");
        self.builder.build_store(var_ptr, elem).unwrap();
        self.compile_stmts(body)?;
        if !self.current_block_terminated() {
            self.builder.build_unconditional_branch(step_bb).unwrap();
        }
        self.loop_stack.pop();

        // Step: increment index and loop back to the condition.
        self.builder.position_at_end(step_bb);
        let one = self.build_int(1);
        let old_idx = self
            .builder
            .build_load(self.cv_type, idx_ptr, "old_idx")
            .unwrap()
            .into_struct_value();
        let new_idx = self.call_binop_fn(self.rt.cool_add, old_idx, one, "add");
        self.builder.build_store(idx_ptr, new_idx).unwrap();
        self.builder.build_unconditional_branch(cond_bb).unwrap();

        self.builder.position_at_end(after_bb);
        Ok(())
    }

    // ── function definition ───────────────────────────────────────────────────

    fn compile_fndef(&mut self, name: &str, params: &[crate::ast::Param], body: &[Stmt]) -> Result<(), String> {
        if !self.allow_toplevel_defs {
            return Err("nested function definitions are not supported in the LLVM backend".into());
        }

        let fn_val = *self
            .functions
            .get(name)
            .ok_or_else(|| format!("function '{name}' was not pre-declared"))?;

        // Save caller state
        let saved_bb = self.builder.get_insert_block();
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_functions = self.functions.clone();
        let saved_function_params = self.function_params.clone();
        let saved_classes = self.classes.clone();
        let saved_fn = self.current_fn.replace(fn_val);
        let saved_loops = std::mem::take(&mut self.loop_stack);
        let saved_imports = std::mem::take(&mut self.imported_modules);
        let saved_user_modules = std::mem::take(&mut self.imported_user_modules);
        let saved_cleanups = std::mem::take(&mut self.cleanup_stack);
        let saved_tries = std::mem::take(&mut self.try_stack);
        let saved_allow_toplevel_defs = self.allow_toplevel_defs;
        self.allow_toplevel_defs = false;
        self.imported_modules = saved_imports.clone();
        self.imported_user_modules = saved_user_modules.clone();

        // Build entry block
        let entry = self.context.append_basic_block(fn_val, "entry");
        self.builder.position_at_end(entry);

        // Bind parameters
        for (i, param) in params.iter().enumerate() {
            if param.is_vararg || param.is_kwarg {
                return Err("*args / **kwargs are not supported in the LLVM backend".into());
            }
            if let Some(param_val) = fn_val.get_nth_param(i as u32) {
                let alloca = self.build_entry_alloca(&param.name);
                self.builder.build_store(alloca, param_val).unwrap();
                self.locals.insert(param.name.clone(), alloca);
            }
        }

        // Compile body
        self.compile_stmts(body)?;

        // Implicit return nil if body didn't terminate
        if !self.current_block_terminated() {
            let nil = self.build_nil();
            self.builder.build_return(Some(&nil)).unwrap();
        }

        // Restore caller state
        self.locals = saved_locals;
        self.functions = saved_functions;
        self.function_params = saved_function_params;
        self.classes = saved_classes;
        self.current_fn = saved_fn;
        self.loop_stack = saved_loops;
        self.imported_modules = saved_imports;
        self.imported_user_modules = saved_user_modules;
        self.cleanup_stack = saved_cleanups;
        self.try_stack = saved_tries;
        self.allow_toplevel_defs = saved_allow_toplevel_defs;
        if let Some(bb) = saved_bb {
            self.builder.position_at_end(bb);
        }

        // Bind the top-level function name as a first-class zero-capture closure
        // so module helpers like list.map(fn, xs) can receive it as a value.
        let fn_ptr = fn_val.as_global_value().as_pointer_value();
        let fn_ptr_int = self
            .builder
            .build_ptr_to_int(fn_ptr, self.context.i64_type(), &format!("{}_fn_ptr", name))
            .unwrap();
        let null_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
        let zero_captures = self.context.i64_type().const_zero();
        let closure = self
            .builder
            .build_call(
                self.rt.cool_closure_new,
                &[fn_ptr_int.into(), zero_captures.into(), null_ptr.into()],
                &format!("{}_closure", name),
            )
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();
        let ptr = if let Some(&p) = self.locals.get(name) {
            p
        } else {
            let p = self.build_entry_alloca(name);
            self.locals.insert(name.to_string(), p);
            p
        };
        self.builder.build_store(ptr, closure).unwrap();
        Ok(())
    }

    // ── class definition ─────────────────────────────────────────────────────

    fn compile_class(&mut self, name: &str, parent: Option<&str>, body: &[Stmt]) -> Result<(), String> {
        if !self.allow_toplevel_defs {
            return Err("class definitions are only allowed at the top level".into());
        }

        // Collect method names and check for __init__
        let mut methods: HashMap<String, FunctionValue<'ctx>> = HashMap::new();
        let mut method_params: HashMap<String, Vec<crate::ast::Param>> = HashMap::new();
        let mut has_init = false;
        let mut init_params: Option<Vec<crate::ast::Param>> = None;
        let mut attributes: Vec<(String, Expr)> = Vec::new();

        if let Some(parent_name) = parent {
            if let Some(parent_info) = self.classes.get(parent_name) {
                methods.extend(parent_info.methods.iter().map(|(k, v)| (k.clone(), *v)));
                method_params.extend(parent_info.method_params.iter().map(|(k, v)| (k.clone(), v.clone())));
                attributes.extend(parent_info.attributes.iter().cloned());
            }
        }

        for stmt in body {
            match stmt {
                Stmt::FnDef {
                    name: mname, params, ..
                } => {
                    if mname == "__init__" {
                        has_init = true;
                        init_params = Some(params.clone());
                    }
                    method_params.insert(mname.clone(), params.clone());
                }
                Stmt::Assign { name: aname, value } => {
                    // Instance attribute assignment - strip "self." prefix for storage
                    let attr_name = if aname.starts_with("self.") {
                        aname.strip_prefix("self.").unwrap().to_string()
                    } else {
                        aname.clone()
                    };
                    attributes.push((attr_name, value.clone()));
                }
                _ => {}
            }
        }

        // Create a global string for the class name
        let name_ptr = self
            .builder
            .build_global_string_ptr(name, &format!("class_name_{}", name))
            .unwrap();

        // First, declare stub functions for all methods
        for stmt in body {
            if let Stmt::FnDef {
                name: mname, params, ..
            } = stmt
            {
                let fn_name = self.mangle_global_name(&format!("{}#{}.{}", name, mname, name));
                let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'_>> =
                    params.iter().map(|_| self.cv_type.into()).collect();
                let fn_type = self.cv_type.fn_type(&param_types, false);
                let fn_val = self.module.add_function(&fn_name, fn_type, None);
                methods.insert(mname.clone(), fn_val);
            }
        }

        // Now compile the methods with self type
        for stmt in body {
            if let Stmt::FnDef {
                name: mname,
                params,
                body: mbody,
            } = stmt
            {
                if let Some(&fn_val) = methods.get(mname) {
                    // Save state
                    let saved_bb = self.builder.get_insert_block();
                    let saved_locals = std::mem::take(&mut self.locals);
                    let saved_functions = self.functions.clone();
                    let saved_function_params = self.function_params.clone();
                    let saved_classes = self.classes.clone();
                    let saved_fn = self.current_fn.replace(fn_val);
                    let saved_loops = std::mem::take(&mut self.loop_stack);
                    let saved_imports = std::mem::take(&mut self.imported_modules);
                    let saved_user_modules = std::mem::take(&mut self.imported_user_modules);
                    let saved_cleanups = std::mem::take(&mut self.cleanup_stack);
                    let saved_tries = std::mem::take(&mut self.try_stack);
                    let saved_class = self.current_class.replace(name.to_string());
                    let saved_allow_toplevel_defs = self.allow_toplevel_defs;
                    self.allow_toplevel_defs = false;
                    self.imported_modules = saved_imports.clone();
                    self.imported_user_modules = saved_user_modules.clone();

                    // Build entry
                    let entry = self.context.append_basic_block(fn_val, "entry");
                    self.builder.position_at_end(entry);

                    // Bind params directly from the LLVM function signature.
                    for (i, param) in params.iter().enumerate() {
                        if param.is_vararg || param.is_kwarg {
                            return Err("*args / **kwargs not supported in methods".into());
                        }
                        if let Some(param_val) = fn_val.get_nth_param(i as u32) {
                            let alloca = self.build_entry_alloca(&param.name);
                            self.builder.build_store(alloca, param_val).unwrap();
                            self.locals.insert(param.name.clone(), alloca);
                        }
                    }

                    // Compile body
                    self.compile_stmts(mbody)?;

                    // Implicit return nil
                    if !self.current_block_terminated() {
                        let nil = self.build_nil();
                        self.builder.build_return(Some(&nil)).unwrap();
                    }

                    // Restore state
                    self.locals = saved_locals;
                    self.functions = saved_functions;
                    self.function_params = saved_function_params;
                    self.classes = saved_classes;
                    self.current_fn = saved_fn;
                    self.loop_stack = saved_loops;
                    self.imported_modules = saved_imports;
                    self.imported_user_modules = saved_user_modules;
                    self.cleanup_stack = saved_cleanups;
                    self.try_stack = saved_tries;
                    self.current_class = saved_class;
                    self.allow_toplevel_defs = saved_allow_toplevel_defs;
                    if let Some(bb) = saved_bb {
                        self.builder.position_at_end(bb);
                    }
                }
            }
        }

        // Build the constructor function
        let ctor_name = self.mangle_global_name(&format!("{}#constructor.{}", name, name));
        let ctor_type = self.cv_type.fn_type(&[], false);
        let constructor = self.module.add_function(&ctor_name, ctor_type, None);

        // Build constructor body
        let saved_bb = self.builder.get_insert_block();
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_fn = self.current_fn.replace(constructor);
        let saved_loops = std::mem::take(&mut self.loop_stack);
        let saved_cleanups = std::mem::take(&mut self.cleanup_stack);
        let saved_tries = std::mem::take(&mut self.try_stack);
        let saved_allow_toplevel_defs = self.allow_toplevel_defs;
        self.allow_toplevel_defs = false;

        let entry = self.context.append_basic_block(constructor, "entry");
        self.builder.position_at_end(entry);

        if let Some(parent_name) = parent {
            let parent_name_ptr = self
                .builder
                .build_global_string_ptr(parent_name, &format!("class_parent_name_{}_{}", name, parent_name))
                .unwrap();
            self.builder
                .build_call(
                    self.rt.cool_register_class_parent,
                    &[
                        name_ptr.as_pointer_value().into(),
                        parent_name_ptr.as_pointer_value().into(),
                    ],
                    "register_class_parent",
                )
                .unwrap();
        }

        // Build method data array: [name_ptr1, fn_ptr1, name_ptr2, fn_ptr2, ...]
        let method_count = methods.len() as i64;

        // Allocate array for method data (2 i64 values per method: name ptr and fn ptr)
        let method_data_size = method_count * 2 * 8; // 2 * i64 per method
        let method_data_size_val = self.build_int(method_data_size);
        let method_data_ptr = self
            .builder
            .build_call(self.rt.cool_malloc, &[method_data_size_val.into()], "method_data_ptr")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();

        // Get the raw pointer
        let method_data_int = self
            .builder
            .build_extract_value(method_data_ptr, 1, "method_data_int")
            .unwrap()
            .into_int_value();
        let method_data_i8ptr = self
            .builder
            .build_int_to_ptr(
                method_data_int,
                self.context.i8_type().ptr_type(inkwell::AddressSpace::default()),
                "method_data_i8ptr",
            )
            .unwrap();

        // Fill in method data
        for (i, (method_name, &fn_val)) in methods.iter().enumerate() {
            let idx = i as u64;

            // Store name pointer at offset idx * 16
            let name_offset = self.context.i64_type().const_int(idx * 16, false);
            let name_ptr_pos = unsafe {
                self.builder
                    .build_in_bounds_gep(
                        self.context.i8_type(),
                        method_data_i8ptr,
                        &[name_offset],
                        "name_ptr_pos",
                    )
                    .unwrap()
            };
            let name_ptr_cast = self
                .builder
                .build_pointer_cast(
                    name_ptr_pos,
                    self.context.i64_type().ptr_type(inkwell::AddressSpace::default()),
                    "name_ptr_cast",
                )
                .unwrap();
            let attr_name = format!("method_{}", method_name);
            let method_name_ptr = self.builder.build_global_string_ptr(&attr_name, &attr_name).unwrap();
            let name_as_int = self
                .builder
                .build_ptr_to_int(method_name_ptr.as_pointer_value(), self.context.i64_type(), "name_int")
                .unwrap();
            self.builder.build_store(name_ptr_cast, name_as_int).unwrap();

            // Store function pointer at offset idx * 16 + 8
            let fn_offset = self.context.i64_type().const_int(idx * 16 + 8, false);
            let fn_ptr_pos = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), method_data_i8ptr, &[fn_offset], "fn_ptr_pos")
                    .unwrap()
            };
            let fn_ptr_cast = self
                .builder
                .build_pointer_cast(
                    fn_ptr_pos,
                    self.context.i64_type().ptr_type(inkwell::AddressSpace::default()),
                    "fn_ptr_cast",
                )
                .unwrap();
            let fn_ptr = fn_val.as_global_value().as_pointer_value();
            let fn_as_int = self
                .builder
                .build_ptr_to_int(fn_ptr, self.context.i64_type(), "fn_int")
                .unwrap();
            self.builder.build_store(fn_ptr_cast, fn_as_int).unwrap();
        }

        // Create class with method data
        let method_data_i64ptr = self
            .builder
            .build_pointer_cast(
                method_data_i8ptr,
                self.context.i64_type().ptr_type(inkwell::AddressSpace::default()),
                "method_data_i64ptr",
            )
            .unwrap();
        let parent_class_val = self.build_nil();
        let class_val = self
            .builder
            .build_call(
                self.rt.cool_class_new,
                &[
                    name_ptr.as_pointer_value().into(),
                    parent_class_val.into(),
                    self.context.i64_type().const_int(method_count as u64, false).into(),
                    method_data_i64ptr.into(),
                ],
                "class",
            )
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();

        let obj_val = self
            .builder
            .build_call(self.rt.cool_object_new, &[class_val.into()], "obj")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();

        // Store object in a temporary for __init__ call
        let obj_ptr = self.builder.build_alloca(self.cv_type, "obj_tmp").unwrap();
        self.builder.build_store(obj_ptr, obj_val).unwrap();

        // Allocate self pointer for __init__ and attribute setting
        let self_ptr = self.builder.build_alloca(self.cv_type, "self_for_init").unwrap();
        self.builder.build_store(self_ptr, obj_val).unwrap();

        // Call __init__ if present
        if has_init || methods.contains_key("__init__") {
            let params = init_params
                .clone()
                .or_else(|| method_params.get("__init__").cloned())
                .unwrap_or_default();

            if let Some(&init_fn) = methods.get("__init__") {
                let mut init_args = vec![obj_val];
                for (i, param) in params.iter().skip(1).enumerate() {
                    let idx = self.context.i32_type().const_int(i as u64, false);
                    let arg_val = self
                        .builder
                        .build_call(self.rt.cool_get_arg, &[idx.into()], &param.name)
                        .unwrap()
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_struct_value();
                    init_args.push(arg_val);
                }
                self.call_fn_with_struct_args(init_fn, &init_args, "init_call");
            }
        }

        // Return the object
        let result = self
            .builder
            .build_load(self.cv_type, obj_ptr, "result")
            .unwrap()
            .into_struct_value();
        self.builder.build_return(Some(&result)).unwrap();

        // Restore state
        self.locals = saved_locals;
        self.current_fn = saved_fn;
        self.loop_stack = saved_loops;
        self.cleanup_stack = saved_cleanups;
        self.try_stack = saved_tries;
        self.allow_toplevel_defs = saved_allow_toplevel_defs;
        if let Some(bb) = saved_bb {
            self.builder.position_at_end(bb);
        }

        let constructor_params = init_params
            .clone()
            .or_else(|| method_params.get("__init__").cloned())
            .unwrap_or_default()
            .into_iter()
            .skip(1)
            .collect();

        // Store class info
        self.classes.insert(
            name.to_string(),
            ClassInfo {
                constructor,
                methods,
                method_params,
                attributes,
                parent: parent.map(str::to_string),
                constructor_params,
            },
        );

        // Create a global variable to hold the class reference
        let global_name = self.mangle_global_name(&format!("__class_{}", name));
        let _global = self.module.add_global(self.cv_type, None, &global_name);

        // At runtime, we need to initialize this - for now, just store constructor ref
        let _constructor_holder = self
            .builder
            .build_alloca(self.cv_type, &format!("{}_holder", name))
            .unwrap();

        let ctor_ptr = constructor.as_global_value().as_pointer_value();
        let ctor_ptr_int = self
            .builder
            .build_ptr_to_int(ctor_ptr, self.context.i64_type(), &format!("{}_ctor_ptr", name))
            .unwrap();
        let null_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
        let zero_captures = self.context.i64_type().const_zero();
        let class_ctor = self
            .builder
            .build_call(
                self.rt.cool_closure_new,
                &[ctor_ptr_int.into(), zero_captures.into(), null_ptr.into()],
                &format!("{}_ctor_closure", name),
            )
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();
        let class_ptr = if let Some(&p) = self.locals.get(name) {
            p
        } else {
            let p = self.build_entry_alloca(name);
            self.locals.insert(name.to_string(), p);
            p
        };
        self.builder.build_store(class_ptr, class_ctor).unwrap();

        // Store class info for later instantiation
        Ok(())
    }

    // ── assert ────────────────────────────────────────────────────────────────

    fn compile_assert(&mut self, condition: &Expr, message: Option<&Expr>) -> Result<(), String> {
        let fn_val = self.current_fn.unwrap();
        let cond_cv = self.compile_expr(condition)?;
        let i1 = self.truthy_i1(cond_cv);

        let ok_bb = self.context.append_basic_block(fn_val, "assert_ok");
        let fail_bb = self.context.append_basic_block(fn_val, "assert_fail");
        self.builder.build_conditional_branch(i1, ok_bb, fail_bb).unwrap();

        // Failure path: print message and abort
        self.builder.position_at_end(fail_bb);
        let msg_cv = if let Some(msg_expr) = message {
            self.compile_expr(msg_expr)?
        } else {
            self.build_str("AssertionError")
        };
        let n1 = self.context.i32_type().const_int(1, false);
        self.builder
            .build_call(self.rt.cool_print, &[n1.into(), msg_cv.into()], "")
            .unwrap();
        self.builder.build_call(self.rt.abort_fn, &[], "").unwrap();
        self.builder.build_unreachable().unwrap();

        self.builder.position_at_end(ok_bb);
        Ok(())
    }

    // ── with / context manager ───────────────────────────────────────────────

    fn compile_with(&mut self, expr: &Expr, as_name: Option<&str>, body: &[Stmt]) -> Result<(), String> {
        let manager_name = format!("__with_manager_{}", self.fresh_name());
        let manager_ptr = self.build_entry_alloca(&manager_name);
        let manager_val = self.compile_expr(expr)?;
        self.builder.build_store(manager_ptr, manager_val).unwrap();

        let entered = self.call_method_named(manager_val, "__enter__", &[], "with_enter");
        self.builder
            .build_call(self.rt.cool_push_with, &[manager_val.into()], "with_push")
            .unwrap();
        if let Some(name) = as_name {
            let ptr = if let Some(&p) = self.locals.get(name) {
                p
            } else {
                let p = self.build_entry_alloca(name);
                self.locals.insert(name.to_string(), p);
                p
            };
            self.builder.build_store(ptr, entered).unwrap();
        }

        self.cleanup_stack.push(CleanupEntry::With { manager_ptr });
        self.compile_stmts(body)?;
        self.cleanup_stack.pop();
        if !self.current_block_terminated() {
            self.emit_with_exit_call(manager_ptr);
        }
        Ok(())
    }

    // ── try / except / else / finally ────────────────────────────────────────
    fn compile_try(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        else_body: Option<&[Stmt]>,
        finally_body: Option<&[Stmt]>,
    ) -> Result<(), String> {
        let fn_val = self.current_fn.unwrap();
        let try_cleanup_depth = self.current_cleanup_depth();
        if let Some(finally) = finally_body {
            self.cleanup_stack
                .push(CleanupEntry::Finally { body: finally.to_vec() });
        }
        self.try_stack.push(TryContext {
            cleanup_depth: try_cleanup_depth,
            catches_exceptions: true,
        });

        let jmp_name = format!("__try_jmp_{}", self.fresh_name());
        let jmp_buf_ptr = self.build_entry_jmp_buf_alloca(&jmp_name);
        let jmp_buf_i8ptr = self
            .builder
            .build_pointer_cast(
                jmp_buf_ptr,
                self.context.i8_type().ptr_type(inkwell::AddressSpace::default()),
                "try_jmp_buf",
            )
            .unwrap();
        self.builder
            .build_call(self.rt.cool_enter_try, &[jmp_buf_i8ptr.into()], "push_try")
            .unwrap();
        let result = self
            .builder
            .build_call(self.rt.setjmp_fn, &[jmp_buf_i8ptr.into()], "setjmp")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value();
        let zero = self.context.i32_type().const_int(0, false);
        let caught_i1 = self
            .builder
            .build_int_compare(IntPredicate::NE, result, zero, "caught")
            .unwrap();

        let try_bb = self.context.append_basic_block(fn_val, "try_body");
        let caught_bb = self.context.append_basic_block(fn_val, "try_caught");
        let after_body_bb = self.context.append_basic_block(fn_val, "try_after_body");
        let merge_bb = self.context.append_basic_block(fn_val, "try_merge");
        let else_bb = else_body.map(|_| self.context.append_basic_block(fn_val, "try_else"));
        let finally_normal_bb = finally_body.map(|_| self.context.append_basic_block(fn_val, "try_finally_normal"));
        let finally_reraise_bb = finally_body.map(|_| self.context.append_basic_block(fn_val, "try_finally_reraise"));
        let no_match_bb = self.context.append_basic_block(fn_val, "try_no_match");
        let exc_name = format!("__try_exc_{}", self.fresh_name());
        let exc_ptr = self.build_entry_alloca(&exc_name);

        self.builder
            .build_conditional_branch(caught_i1, caught_bb, try_bb)
            .unwrap();

        self.builder.position_at_end(try_bb);
        self.compile_stmts(body)?;
        if !self.current_block_terminated() {
            self.builder
                .build_call(self.rt.cool_exit_try, &[], "exit_try_normal")
                .unwrap();
            if let Some(bb) = else_bb {
                self.builder.build_unconditional_branch(bb).unwrap();
            } else {
                self.builder.build_unconditional_branch(after_body_bb).unwrap();
            }
        }

        if let Some(ctx) = self.try_stack.last_mut() {
            ctx.catches_exceptions = false;
        }

        if let Some(bb) = else_bb {
            self.builder.position_at_end(bb);
            if let Some(stmts) = else_body {
                self.compile_stmts(stmts)?;
            }
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(after_body_bb).unwrap();
            }
        }

        self.builder.position_at_end(caught_bb);
        self.builder
            .build_call(self.rt.cool_exit_try, &[], "exit_try_caught")
            .unwrap();
        let exc_val = self
            .builder
            .build_call(self.rt.cool_get_exception, &[], "get_exc")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();
        self.builder.build_store(exc_ptr, exc_val).unwrap();

        let mut next_check_bb = caught_bb;
        for (idx, handler) in handlers.iter().enumerate() {
            let handler_entry_bb = self.context.append_basic_block(fn_val, &format!("handler_{idx}"));
            self.builder.position_at_end(next_check_bb);

            if let Some(exc_type) = &handler.exc_type {
                let next_bb = if idx + 1 == handlers.len() {
                    no_match_bb
                } else {
                    self.context
                        .append_basic_block(fn_val, &format!("handler_check_{}", idx + 1))
                };
                let exc_cur = self
                    .builder
                    .build_load(self.cv_type, exc_ptr, "exc_for_match")
                    .unwrap()
                    .into_struct_value();
                let type_ptr = self
                    .builder
                    .build_global_string_ptr(exc_type, &format!("exc_type_{}_{}", exc_type, idx))
                    .unwrap();
                let matched_i32 = self
                    .builder
                    .build_call(
                        self.rt.cool_exception_matches,
                        &[exc_cur.into(), type_ptr.as_pointer_value().into()],
                        "exc_matches",
                    )
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();
                let matched = self
                    .builder
                    .build_int_compare(IntPredicate::NE, matched_i32, zero, "exc_match_bool")
                    .unwrap();
                self.builder
                    .build_conditional_branch(matched, handler_entry_bb, next_bb)
                    .unwrap();
                next_check_bb = next_bb;
            } else {
                self.builder.build_unconditional_branch(handler_entry_bb).unwrap();
                next_check_bb = no_match_bb;
            }

            self.builder.position_at_end(handler_entry_bb);
            if let Some(as_name) = &handler.as_name {
                let ptr = if let Some(&p) = self.locals.get(as_name) {
                    p
                } else {
                    let p = self.build_entry_alloca(as_name);
                    self.locals.insert(as_name.clone(), p);
                    p
                };
                let exc_cur = self
                    .builder
                    .build_load(self.cv_type, exc_ptr, "exc_for_bind")
                    .unwrap()
                    .into_struct_value();
                self.builder.build_store(ptr, exc_cur).unwrap();
            }

            self.compile_stmts(&handler.body)?;
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(after_body_bb).unwrap();
            }

            if handler.exc_type.is_none() {
                break;
            }
        }

        if next_check_bb != no_match_bb {
            self.builder.position_at_end(next_check_bb);
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(no_match_bb).unwrap();
            }
        }

        self.try_stack.pop();
        if finally_body.is_some() {
            self.cleanup_stack.pop();
        }

        self.builder.position_at_end(after_body_bb);
        if !self.current_block_terminated() {
            if let Some(bb) = finally_normal_bb {
                self.builder.build_unconditional_branch(bb).unwrap();
            } else {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }
        }

        self.builder.position_at_end(no_match_bb);
        if let Some(bb) = finally_reraise_bb {
            self.builder.build_unconditional_branch(bb).unwrap();
        } else {
            let exc_cur = self
                .builder
                .build_load(self.cv_type, exc_ptr, "exc_for_reraise")
                .unwrap()
                .into_struct_value();
            self.builder
                .build_call(self.rt.cool_raise, &[exc_cur.into()], "re_raise")
                .unwrap();
            self.builder.build_unreachable().unwrap();
        }

        if let Some(finally) = finally_body {
            let finally_normal_bb = finally_normal_bb.expect("finally block should exist");
            self.builder.position_at_end(finally_normal_bb);
            self.compile_stmts(finally)?;
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }

            let finally_reraise_bb = finally_reraise_bb.expect("finally rethrow block should exist");
            self.builder.position_at_end(finally_reraise_bb);
            self.compile_stmts(finally)?;
            if !self.current_block_terminated() {
                let exc_cur = self
                    .builder
                    .build_load(self.cv_type, exc_ptr, "exc_after_finally")
                    .unwrap()
                    .into_struct_value();
                self.builder
                    .build_call(self.rt.cool_raise, &[exc_cur.into()], "raise_after_finally")
                    .unwrap();
                self.builder.build_unreachable().unwrap();
            }
        }

        self.builder.position_at_end(merge_bb);
        Ok(())
    }

    // ── raise ────────────────────────────────────────────────────────────────
    fn compile_raise(&mut self, opt_expr: Option<&Expr>) -> Result<(), String> {
        let exc_val = if let Some(e) = opt_expr {
            self.compile_expr(e)?
        } else {
            self.builder
                .build_call(self.rt.cool_get_exception, &[], "raise_current_exc")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value()
        };

        if let Some(cleanup_depth) = self.current_raise_cleanup_depth() {
            self.emit_cleanup_from_depth(cleanup_depth)?;
            if self.current_block_terminated() {
                return Ok(());
            }
        }

        self.builder
            .build_call(self.rt.cool_raise, &[exc_val.into()], "raise")
            .unwrap();
        self.builder.build_unreachable().unwrap();
        Ok(())
    }

    fn call_constructor(
        &mut self,
        constructor: FunctionValue<'ctx>,
        ctor_params: &[crate::ast::Param],
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<StructValue<'ctx>, String> {
        let compiled = self.bind_call_args(ctor_params, args, kwargs, 0)?;
        let i32t = self.context.i32_type();
        for (i, cv) in compiled.iter().enumerate() {
            let idx_val = i32t.const_int(i as u64, false);
            self.builder
                .build_call(
                    self.rt.cool_set_global_arg,
                    &[idx_val.into(), (*cv).into()],
                    "set_global_arg",
                )
                .unwrap();
        }
        Ok(self
            .builder
            .build_call(constructor, &[], "instantiate")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value())
    }

    fn compile_module_function(&mut self, module_path: &Path) -> Result<ModuleInfo<'ctx>, String> {
        let canonical_path = module_path.canonicalize().unwrap_or_else(|_| module_path.to_path_buf());
        if let Some(info) = self.compiled_modules.get(&canonical_path) {
            return Ok(info.clone());
        }
        if self.compiling_modules.contains(&canonical_path) {
            return Err(format!("circular import detected for '{}'", canonical_path.display()));
        }

        let source = std::fs::read_to_string(&canonical_path)
            .map_err(|e| format!("import {}: {}", canonical_path.display(), e))?;
        let mut lexer = crate::lexer::Lexer::new(&source);
        let tokens = lexer.tokenize().map_err(|e| format!("import parse error: {}", e))?;
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser
            .parse_program()
            .map_err(|e| format!("import parse error: {}", e))?;

        self.compiling_modules.push(canonical_path.clone());

        let init_base = format!("__module_init_{}", self.fresh_name());
        let init_name = self.mangle_global_name(&init_base);
        let init_fn = self
            .module
            .add_function(&init_name, self.cv_type.fn_type(&[], false), None);

        let saved_bb = self.builder.get_insert_block();
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_functions = std::mem::take(&mut self.functions);
        let saved_function_params = std::mem::take(&mut self.function_params);
        let saved_classes = std::mem::take(&mut self.classes);
        let saved_fn = self.current_fn.replace(init_fn);
        let saved_loops = std::mem::take(&mut self.loop_stack);
        let saved_imports = std::mem::take(&mut self.imported_modules);
        let saved_user_modules = std::mem::take(&mut self.imported_user_modules);
        let saved_cleanups = std::mem::take(&mut self.cleanup_stack);
        let saved_tries = std::mem::take(&mut self.try_stack);
        let saved_class = self.current_class.take();
        let saved_source_dir = self.current_source_dir.clone();
        let module_prefix = format!("__mod_{}__", self.fresh_name());
        let saved_symbol_prefix = std::mem::replace(&mut self.symbol_prefix, module_prefix);
        let saved_allow_toplevel_defs = self.allow_toplevel_defs;

        let entry = self.context.append_basic_block(init_fn, "entry");
        self.builder.position_at_end(entry);
        self.current_source_dir = canonical_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        self.allow_toplevel_defs = true;

        let compile_result = (|| -> Result<ModuleInfo<'ctx>, String> {
            self.declare_top_level_functions(&program)?;
            self.compile_stmts(&program)?;

            let mut exports: Vec<String> = self.locals.keys().cloned().collect();
            exports.sort();

            if !self.current_block_terminated() {
                let namespace = self
                    .builder
                    .build_call(self.rt.cool_dict_new, &[], "module_namespace")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value();
                for export in &exports {
                    let value = self
                        .builder
                        .build_load(
                            self.cv_type,
                            *self.locals.get(export).expect("module export local missing"),
                            export,
                        )
                        .unwrap()
                        .into_struct_value();
                    let name_ptr = self
                        .builder
                        .build_global_string_ptr(export, &format!("{}_export_{}", init_name, export))
                        .unwrap();
                    self.builder
                        .build_call(
                            self.rt.cool_set_attr,
                            &[namespace.into(), name_ptr.as_pointer_value().into(), value.into()],
                            "module_export",
                        )
                        .unwrap();
                }
                self.builder.build_return(Some(&namespace)).unwrap();
            }

            Ok(ModuleInfo {
                init_fn,
                exports,
                functions: self.functions.clone(),
                function_params: self.function_params.clone(),
                classes: self.classes.clone(),
            })
        })();

        self.locals = saved_locals;
        self.functions = saved_functions;
        self.function_params = saved_function_params;
        self.classes = saved_classes;
        self.current_fn = saved_fn;
        self.loop_stack = saved_loops;
        self.imported_modules = saved_imports;
        self.imported_user_modules = saved_user_modules;
        self.cleanup_stack = saved_cleanups;
        self.try_stack = saved_tries;
        self.current_class = saved_class;
        self.current_source_dir = saved_source_dir;
        self.symbol_prefix = saved_symbol_prefix;
        self.allow_toplevel_defs = saved_allow_toplevel_defs;
        if let Some(bb) = saved_bb {
            self.builder.position_at_end(bb);
        }

        self.compiling_modules.pop();

        let info = compile_result?;
        self.compiled_modules.insert(canonical_path, info.clone());
        Ok(info)
    }

    // ── import "path.cool" ────────────────────────────────────────────────────
    fn compile_import(&mut self, path: &str) -> Result<(), String> {
        let module_path = self.resolve_import_file_path(path)?;
        let module_info = self.compile_module_function(&module_path)?;
        let namespace = self
            .builder
            .build_call(module_info.init_fn, &[], "import_file")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();

        for export in &module_info.exports {
            let export_name_ptr = self
                .builder
                .build_global_string_ptr(export, &format!("import_file_export_{}", export))
                .unwrap();
            let value = self
                .builder
                .build_call(
                    self.rt.cool_get_attr,
                    &[namespace.into(), export_name_ptr.as_pointer_value().into()],
                    "import_file_attr",
                )
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value();
            let ptr = if let Some(&p) = self.locals.get(export) {
                p
            } else {
                let p = self.build_entry_alloca(export);
                self.locals.insert(export.clone(), p);
                p
            };
            self.builder.build_store(ptr, value).unwrap();
        }
        for (name, &fn_val) in &module_info.functions {
            self.functions.insert(name.clone(), fn_val);
        }
        for (name, params) in &module_info.function_params {
            self.function_params.insert(name.clone(), params.clone());
        }
        for (name, class_info) in &module_info.classes {
            self.classes.insert(name.clone(), class_info.clone());
        }
        Ok(())
    }

    // ── import module_name ────────────────────────────────────────────────────
    fn compile_import_module(&mut self, name: &str) -> Result<(), String> {
        match name {
            "math" | "os" | "sys" | "subprocess" | "argparse" | "logging" | "csv" | "test" | "time" | "random"
            | "json" | "string" | "list" | "re" | "collections" | "path" | "ffi" => {
                self.imported_modules.insert(name.to_string());
                let module_val = self.build_str(&format!("<module {}>", name));
                let ptr = self.build_entry_alloca(name);
                self.builder.build_store(ptr, module_val).unwrap();
                self.locals.insert(name.to_string(), ptr);
                Ok(())
            }
            _ => {
                let module_path = self
                    .resolve_import_module_path(name)
                    .ok_or_else(|| format!("import: unknown module '{}'", name))?;
                let module_info = self.compile_module_function(&module_path)?;
                let namespace = self
                    .builder
                    .build_call(module_info.init_fn, &[], "import_module")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value();
                let binding_name = name.rsplit('.').next().unwrap_or(name);
                let ptr = if let Some(&p) = self.locals.get(binding_name) {
                    p
                } else {
                    let p = self.build_entry_alloca(binding_name);
                    self.locals.insert(binding_name.to_string(), p);
                    p
                };
                self.builder.build_store(ptr, namespace).unwrap();
                self.imported_user_modules.insert(binding_name.to_string(), module_path);
                Ok(())
            }
        }
    }

    // ── Expression compiler ───────────────────────────────────────────────────

    fn compile_expr(&mut self, expr: &Expr) -> Result<StructValue<'ctx>, String> {
        match expr {
            Expr::Nil => Ok(self.build_nil()),
            Expr::Int(n) => Ok(self.build_int(*n)),
            Expr::Float(f) => Ok(self.build_float(*f)),
            Expr::Bool(b) => Ok(self.build_bool(*b)),
            Expr::Str(s) => Ok(self.build_str(s)),

            Expr::Ident(name) => {
                let ptr = self
                    .locals
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("undefined variable '{name}'"))?;
                Ok(self
                    .builder
                    .build_load(self.cv_type, ptr, name)
                    .unwrap()
                    .into_struct_value())
            }

            Expr::BinOp { op, left, right } => self.compile_binop_expr(op, left, right),

            Expr::UnaryOp { op, expr } => {
                let v = self.compile_expr(expr)?;
                let fn_val = match op {
                    UnaryOp::Neg => self.rt.cool_neg,
                    UnaryOp::Not => self.rt.cool_not,
                    UnaryOp::BitNot => self.rt.cool_bitnot,
                };
                Ok(self.call_unop_fn(fn_val, v, "unop"))
            }

            Expr::Call { callee, args, kwargs } => self.compile_call(callee, args, kwargs),

            // list literal: [a, b, c]
            Expr::List(elems) => {
                let n_i64 = self.build_int(elems.len() as i64);
                let list_val = self.call_unop_fn(self.rt.cool_list_make, n_i64, "list");
                for elem in elems {
                    let elem_val = self.compile_expr(elem)?;
                    self.call_binop_fn(self.rt.cool_list_push, list_val, elem_val, "push");
                }
                Ok(list_val)
            }

            // f-string: f"Hello {name}!"
            Expr::FString(parts) => {
                // Create empty string as starting point
                let empty_str_ptr = self.builder.build_global_string_ptr("", "empty").unwrap();
                let mut result = self
                    .builder
                    .build_call(self.rt.cv_str, &[empty_str_ptr.as_pointer_value().into()], "cv_str")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value();

                for part in parts {
                    match part {
                        FStringPart::Literal(s) => {
                            // Create string literal value
                            let str_ptr = self
                                .builder
                                .build_global_string_ptr(s, &format!("lit_{}", self.str_counter))
                                .unwrap();
                            self.str_counter += 1;
                            let str_val = self
                                .builder
                                .build_call(self.rt.cv_str, &[str_ptr.as_pointer_value().into()], "cv_str")
                                .unwrap()
                                .try_as_basic_value()
                                .left()
                                .unwrap()
                                .into_struct_value();
                            // Concatenate with result using +
                            result = self.call_binop_fn(self.rt.cool_add, result, str_val, "add");
                        }
                        FStringPart::Expr(e) => {
                            // Evaluate expression
                            let expr_val = self.compile_expr(e)?;
                            // Convert to string
                            let str_ptr_call = self
                                .builder
                                .build_call(self.rt.cool_to_str, &[expr_val.into()], "to_str")
                                .unwrap();
                            let str_ptr = str_ptr_call.try_as_basic_value().left().unwrap().into_pointer_value();
                            // Create CoolVal from string pointer
                            let str_val = self
                                .builder
                                .build_call(self.rt.cv_str, &[str_ptr.into()], "cv_str")
                                .unwrap()
                                .try_as_basic_value()
                                .left()
                                .unwrap()
                                .into_struct_value();
                            // Concatenate
                            result = self.call_binop_fn(self.rt.cool_add, result, str_val, "add");
                        }
                    }
                }
                Ok(result)
            }

            // index access: obj[i] — works for lists and dicts
            Expr::Index { object, index } => {
                let obj_val = self.compile_expr(object)?;
                let idx_val = self.compile_expr(index)?;
                Ok(self.call_binop_fn(self.rt.cool_index, obj_val, idx_val, "index"))
            }

            Expr::Slice { object, start, stop } => {
                let obj_val = self.compile_expr(object)?;
                let start_val = match start {
                    Some(expr) => self.compile_expr(expr)?,
                    None => self.build_nil(),
                };
                let stop_val = match stop {
                    Some(expr) => self.compile_expr(expr)?,
                    None => self.build_nil(),
                };
                Ok(self.call_triop_fn(self.rt.cool_slice, obj_val, start_val, stop_val, "slice"))
            }

            // attribute access: obj.attr
            Expr::Attr { object, name } => {
                if let Expr::Ident(module_name) = object.as_ref() {
                    if self.imported_modules.contains(module_name) {
                        let module_ptr = self
                            .builder
                            .build_global_string_ptr(module_name, &format!("module_{}_name", module_name))
                            .unwrap();
                        let attr_ptr = self
                            .builder
                            .build_global_string_ptr(name, &format!("module_{}_attr_{}", module_name, name))
                            .unwrap();
                        return Ok(self
                            .builder
                            .build_call(
                                self.rt.cool_module_get_attr,
                                &[module_ptr.as_pointer_value().into(), attr_ptr.as_pointer_value().into()],
                                "module_attr",
                            )
                            .unwrap()
                            .try_as_basic_value()
                            .left()
                            .unwrap()
                            .into_struct_value());
                    }
                }
                if let Expr::Ident(class_name) = object.as_ref() {
                    if let Some(class_info) = self.classes.get(class_name) {
                        if let Some((_, expr)) = class_info.attributes.iter().find(|(attr, _)| attr == name) {
                            let expr = expr.clone();
                            return self.compile_expr(&expr);
                        }
                    }
                }
                let obj_val = self.compile_expr(object)?;
                let attr_name_ptr = self
                    .builder
                    .build_global_string_ptr(name, &format!("attr_{}", name))
                    .unwrap();
                Ok(self
                    .builder
                    .build_call(
                        self.rt.cool_get_attr,
                        &[obj_val.into(), attr_name_ptr.as_pointer_value().into()],
                        "get_attr",
                    )
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value())
            }

            // lambda x, y: x + y — creates a closure
            Expr::Lambda { params, body } => {
                // Lambda creates a closure that captures the current environment.
                // We create a function pointer dynamically and bundle it with captured values.
                //
                // Strategy:
                // 1. Pre-declare a unique helper function for this lambda (fn_name by counter)
                // 2. Store captured variables to globals so the helper can read them
                // 3. Create the closure object with function ptr + capture count
                //
                // Since we can't dynamically create LLVM functions at runtime, we:
                // - Create a unique named helper function (compiled later)
                // - Capture all currently-visible locals as global data
                // - Create closure with helper fn ptr and number of captures

                let fn_name = format!("__lambda_{}", self.str_counter);
                self.str_counter += 1;

                // Create the helper function type
                let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'_>> =
                    params.iter().map(|_| self.cv_type.into()).collect();
                let fn_type = self.cv_type.fn_type(&param_types, false);
                let lambda_fn = self.module.add_function(&fn_name, fn_type, None);

                // Store captured variable count
                let num_captures = self.locals.len();
                let captures_i64 = self.context.i64_type().const_int(num_captures as u64, false);

                // Store each local's current value to globals that the lambda can access
                let i32t = self.context.i32_type();
                for (i, (_, ptr)) in self.locals.iter().enumerate() {
                    let val = self
                        .builder
                        .build_load(self.cv_type, *ptr, "capture_load")
                        .unwrap()
                        .into_struct_value();
                    let idx_val = i32t.const_int(i as u64, false);
                    self.builder
                        .build_call(
                            self.rt.cool_set_closure_capture,
                            &[idx_val.into(), val.into()],
                            "set_capture",
                        )
                        .unwrap();
                }

                // Get function pointer as i64 using pointer-to-int cast
                let fn_ptr_val = lambda_fn.as_global_value().as_pointer_value();
                let fn_ptr_int = self
                    .builder
                    .build_ptr_to_int(fn_ptr_val, self.context.i64_type(), "fn_ptr_int")
                    .unwrap();

                // Create null pointer for captures array (we use global storage instead)
                let null_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();

                let closure = self
                    .builder
                    .build_call(
                        self.rt.cool_closure_new,
                        &[fn_ptr_int.into(), captures_i64.into(), null_ptr.into()],
                        "make_closure",
                    )
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value();

                // Store this closure's function for later compilation
                self.nested_functions
                    .push((fn_name.clone(), params.clone(), vec![Stmt::Return(Some(*body.clone()))]));

                // We'll compile nested functions at the end. For now, return the closure.
                // Note: We need to register the function for later compilation.
                // The nested_functions vec handles this.

                Ok(closure)
            }

            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                let fn_val = self.current_fn.unwrap();
                let then_bb = self.context.append_basic_block(fn_val, "tern_then");
                let else_bb = self.context.append_basic_block(fn_val, "tern_else");
                let done_bb = self.context.append_basic_block(fn_val, "tern_done");

                let cond_cv = self.compile_expr(condition)?;
                let i1 = self.truthy_i1(cond_cv);
                self.builder.build_conditional_branch(i1, then_bb, else_bb).unwrap();

                self.builder.position_at_end(then_bb);
                let then_val = self.compile_expr(then_expr)?;
                let then_end = self.builder.get_insert_block().unwrap();
                self.builder.build_unconditional_branch(done_bb).unwrap();

                self.builder.position_at_end(else_bb);
                let else_val = self.compile_expr(else_expr)?;
                let else_end = self.builder.get_insert_block().unwrap();
                self.builder.build_unconditional_branch(done_bb).unwrap();

                self.builder.position_at_end(done_bb);
                let phi = self.builder.build_phi(self.cv_type, "tern").unwrap();
                phi.add_incoming(&[(&then_val, then_end), (&else_val, else_end)]);
                Ok(phi.as_basic_value().into_struct_value())
            }

            Expr::ListComp {
                expr,
                var,
                iter,
                condition,
            } => {
                let fn_val = self.current_fn.unwrap();

                // Allocate result list
                let result_ptr = self.builder.build_alloca(self.cv_type, "lc_result").unwrap();
                let zero_val = self.build_int(0);
                let empty_list = self.call_unop_fn(self.rt.cool_list_make, zero_val, "lc_list");
                self.builder.build_store(result_ptr, empty_list).unwrap();

                // Compile the iterable
                let iter_val = self.compile_expr(iter)?;
                let idx_ptr = self.builder.build_alloca(self.cv_type, "lc_idx").unwrap();
                let idx_zero = self.build_int(0);
                self.builder.build_store(idx_ptr, idx_zero).unwrap();

                let var_ptr = self.build_entry_alloca(var);
                let saved_var = self.locals.get(var).copied();
                self.locals.insert(var.clone(), var_ptr);

                let cond_bb = self.context.append_basic_block(fn_val, "lc_cond");
                let body_bb = self.context.append_basic_block(fn_val, "lc_body");
                let after_bb = self.context.append_basic_block(fn_val, "lc_after");

                self.builder.build_unconditional_branch(cond_bb).unwrap();

                // Condition: idx < len
                self.builder.position_at_end(cond_bb);
                let idx_cv = self
                    .builder
                    .build_load(self.cv_type, idx_ptr, "lc_idx_load")
                    .unwrap()
                    .into_struct_value();
                let len_cv = self.call_unop_fn(self.rt.cool_list_len, iter_val.clone(), "lc_len");
                let lt = self.call_binop_fn(self.rt.cool_lt, idx_cv, len_cv, "lc_lt");
                let i1 = self.truthy_i1(lt);
                self.builder.build_conditional_branch(i1, body_bb, after_bb).unwrap();

                // Body: optionally filter, then push expr
                self.builder.position_at_end(body_bb);
                let elem = self.call_binop_fn(self.rt.cool_list_get, iter_val.clone(), idx_cv, "lc_elem");
                self.builder.build_store(var_ptr, elem).unwrap();

                let push_bb = if let Some(cond_expr) = condition {
                    let skip_bb = self.context.append_basic_block(fn_val, "lc_skip");
                    let push_bb = self.context.append_basic_block(fn_val, "lc_push");
                    let cond_cv = self.compile_expr(cond_expr)?;
                    let ci1 = self.truthy_i1(cond_cv);
                    self.builder.build_conditional_branch(ci1, push_bb, skip_bb).unwrap();
                    self.builder.position_at_end(skip_bb);
                    // Increment idx and loop back
                    let old_idx = self
                        .builder
                        .build_load(self.cv_type, idx_ptr, "lc_skip_idx")
                        .unwrap()
                        .into_struct_value();
                    let one_skip = self.build_int(1);
                    let new_idx = self.call_binop_fn(self.rt.cool_add, old_idx, one_skip, "lc_inc");
                    self.builder.build_store(idx_ptr, new_idx).unwrap();
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                    push_bb
                } else {
                    body_bb
                };

                self.builder.position_at_end(push_bb);
                let push_elem = if condition.is_some() {
                    // re-load var (it was stored before the filter branch)
                    self.builder
                        .build_load(self.cv_type, var_ptr, "lc_var")
                        .unwrap()
                        .into_struct_value();
                    self.compile_expr(expr)?
                } else {
                    self.compile_expr(expr)?
                };
                let result_cv = self
                    .builder
                    .build_load(self.cv_type, result_ptr, "lc_res_load")
                    .unwrap()
                    .into_struct_value();
                self.call_binop_fn(self.rt.cool_list_push, result_cv, push_elem, "lc_push");

                // Increment idx
                let old_idx2 = self
                    .builder
                    .build_load(self.cv_type, idx_ptr, "lc_old_idx")
                    .unwrap()
                    .into_struct_value();
                let one_inc = self.build_int(1);
                let new_idx2 = self.call_binop_fn(self.rt.cool_add, old_idx2, one_inc, "lc_inc2");
                self.builder.build_store(idx_ptr, new_idx2).unwrap();
                self.builder.build_unconditional_branch(cond_bb).unwrap();

                self.builder.position_at_end(after_bb);

                // Restore shadowed variable if any
                match saved_var {
                    Some(ptr) => {
                        self.locals.insert(var.clone(), ptr);
                    }
                    None => {
                        self.locals.remove(var);
                    }
                }

                Ok(self
                    .builder
                    .build_load(self.cv_type, result_ptr, "lc_final")
                    .unwrap()
                    .into_struct_value())
            }

            // tuple literal: (a, b, c)
            Expr::Tuple(elems) => {
                let n_i64 = self.build_int(elems.len() as i64);
                let tup_val = self.call_unop_fn(self.rt.cool_tuple_make, n_i64, "tuple");
                for elem_expr in elems {
                    let elem_val = self.compile_expr(elem_expr)?;
                    self.call_binop_fn(self.rt.cool_list_push, tup_val, elem_val, "tup_push");
                }
                Ok(tup_val)
            }

            // dict literal: {k: v, ...}
            Expr::Dict(pairs) => {
                let dict_val = self
                    .builder
                    .build_call(self.rt.cool_dict_new, &[], "dict")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value();
                let dict_ptr = self.builder.build_alloca(self.cv_type, "dict_tmp").unwrap();
                self.builder.build_store(dict_ptr, dict_val).unwrap();
                for (k_expr, v_expr) in pairs {
                    let k_val = self.compile_expr(k_expr)?;
                    let v_val = self.compile_expr(v_expr)?;
                    let cur = self
                        .builder
                        .build_load(self.cv_type, dict_ptr, "dict_cur")
                        .unwrap()
                        .into_struct_value();
                    let updated = self.call_triop_fn(self.rt.cool_setindex, cur, k_val, v_val, "dict_set");
                    self.builder.build_store(dict_ptr, updated).unwrap();
                }
                Ok(self
                    .builder
                    .build_load(self.cv_type, dict_ptr, "dict_final")
                    .unwrap()
                    .into_struct_value())
            }
        }
    }

    // ── Binary expression ─────────────────────────────────────────────────────

    fn compile_binop_expr(&mut self, op: &BinOp, left: &Expr, right: &Expr) -> Result<StructValue<'ctx>, String> {
        match op {
            BinOp::And => return self.compile_and(left, right),
            BinOp::Or => return self.compile_or(left, right),
            BinOp::In | BinOp::NotIn => {
                let container = self.compile_expr(right)?;
                let item = self.compile_expr(left)?;
                let result = self.call_binop_fn(self.rt.cool_contains, container, item, "contains");
                return if matches!(op, BinOp::NotIn) {
                    Ok(self.call_unop_fn(self.rt.cool_not, result, "not"))
                } else {
                    Ok(result)
                };
            }
            _ => {}
        }

        // Eager evaluation for all other ops
        let a = self.compile_expr(left)?;
        let b = self.compile_expr(right)?;
        self.apply_binop(op, a, b)
    }

    // ── Short-circuit  `a and b` ──────────────────────────────────────────────
    //
    // Semantics: if falsy(a) return a else return b.
    //
    //   current_bb:
    //     %lhs = <compile left>
    //     %i1  = truthy_i1(%lhs)
    //     br i1 %i1, %and_rhs, %and_done
    //   and_rhs:
    //     %rhs = <compile right>
    //     br %and_done
    //   and_done:
    //     %result = phi CoolVal [ %lhs, %lhs_end ], [ %rhs, %rhs_end ]

    fn compile_and(&mut self, left: &Expr, right: &Expr) -> Result<StructValue<'ctx>, String> {
        let fn_val = self.current_fn.unwrap();

        let lhs = self.compile_expr(left)?;
        let lhs_end = self.builder.get_insert_block().unwrap();
        let i1 = self.truthy_i1(lhs);

        let rhs_bb = self.context.append_basic_block(fn_val, "and_rhs");
        let done_bb = self.context.append_basic_block(fn_val, "and_done");
        self.builder.build_conditional_branch(i1, rhs_bb, done_bb).unwrap();

        self.builder.position_at_end(rhs_bb);
        let rhs = self.compile_expr(right)?;
        let rhs_end = self.builder.get_insert_block().unwrap();
        self.builder.build_unconditional_branch(done_bb).unwrap();

        self.builder.position_at_end(done_bb);
        let phi = self.builder.build_phi(self.cv_type, "and_res").unwrap();
        phi.add_incoming(&[(&lhs as &dyn BasicValue, lhs_end), (&rhs as &dyn BasicValue, rhs_end)]);
        Ok(phi.as_basic_value().into_struct_value())
    }

    // ── Short-circuit  `a or b` ───────────────────────────────────────────────
    //
    // Semantics: if truthy(a) return a else return b.

    fn compile_or(&mut self, left: &Expr, right: &Expr) -> Result<StructValue<'ctx>, String> {
        let fn_val = self.current_fn.unwrap();

        let lhs = self.compile_expr(left)?;
        let lhs_end = self.builder.get_insert_block().unwrap();
        let i1 = self.truthy_i1(lhs);

        let rhs_bb = self.context.append_basic_block(fn_val, "or_rhs");
        let done_bb = self.context.append_basic_block(fn_val, "or_done");
        // truthy → skip rhs (return lhs), falsy → evaluate rhs
        self.builder.build_conditional_branch(i1, done_bb, rhs_bb).unwrap();

        self.builder.position_at_end(rhs_bb);
        let rhs = self.compile_expr(right)?;
        let rhs_end = self.builder.get_insert_block().unwrap();
        self.builder.build_unconditional_branch(done_bb).unwrap();

        self.builder.position_at_end(done_bb);
        let phi = self.builder.build_phi(self.cv_type, "or_res").unwrap();
        phi.add_incoming(&[(&lhs as &dyn BasicValue, lhs_end), (&rhs as &dyn BasicValue, rhs_end)]);
        Ok(phi.as_basic_value().into_struct_value())
    }

    fn bind_call_args(
        &mut self,
        params: &[crate::ast::Param],
        args: &[Expr],
        kwargs: &[(String, Expr)],
        skip_leading: usize,
    ) -> Result<Vec<StructValue<'ctx>>, String> {
        if params.iter().any(|p| p.is_vararg || p.is_kwarg) {
            return Err("varargs and kwargs are not supported in the LLVM backend".into());
        }
        let effective = &params[skip_leading..];
        if args.len() > effective.len() {
            return Err("too many positional arguments".into());
        }

        let mut bound: Vec<Option<StructValue<'ctx>>> = vec![None; effective.len()];
        for (i, arg) in args.iter().enumerate() {
            bound[i] = Some(self.compile_expr(arg)?);
        }

        for (name, expr) in kwargs {
            let pos = effective
                .iter()
                .position(|p| p.name == *name)
                .ok_or_else(|| format!("unknown keyword argument '{name}'"))?;
            if bound[pos].is_some() {
                return Err(format!("multiple values for argument '{name}'"));
            }
            bound[pos] = Some(self.compile_expr(expr)?);
        }

        let mut out = Vec::with_capacity(effective.len());
        for (i, param) in effective.iter().enumerate() {
            if let Some(v) = bound[i] {
                out.push(v);
            } else if let Some(default) = &param.default {
                out.push(self.compile_expr(default)?);
            } else {
                return Err(format!("missing required argument '{}'", param.name));
            }
        }
        Ok(out)
    }

    fn call_fn_with_struct_args(
        &mut self,
        fn_val: FunctionValue<'ctx>,
        values: &[StructValue<'ctx>],
        name: &str,
    ) -> StructValue<'ctx> {
        let args: Vec<BasicMetadataValueEnum<'ctx>> = values.iter().map(|v| (*v).into()).collect();
        self.builder
            .build_call(fn_val, &args, name)
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    fn call_ffi_value(
        &mut self,
        callable: StructValue<'ctx>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
        name: &str,
    ) -> Result<StructValue<'ctx>, String> {
        if !kwargs.is_empty() {
            return Err("FFI functions do not support keyword arguments".into());
        }
        let nargs_i32 = self.context.i32_type().const_int(args.len() as u64, false);
        let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![callable.into(), nargs_i32.into()];
        for arg in args {
            call_args.push(self.compile_expr(arg)?.into());
        }
        Ok(self
            .builder
            .build_call(self.rt.cool_ffi_call, &call_args, name)
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value())
    }

    // ── Function call ─────────────────────────────────────────────────────────

    fn compile_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<StructValue<'ctx>, String> {
        if let Expr::Attr { object, name: member } = callee {
            if let Expr::Ident(module_name) = object.as_ref() {
                if self.imported_modules.contains(module_name) {
                    let module_ptr = self
                        .builder
                        .build_global_string_ptr(module_name, &format!("module_call_{}_name", module_name))
                        .unwrap();
                    let member_ptr = self
                        .builder
                        .build_global_string_ptr(member, &format!("module_call_{}_{}", module_name, member))
                        .unwrap();
                    let nargs_i32 = self.context.i32_type().const_int(args.len() as u64, false);
                    let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![
                        module_ptr.as_pointer_value().into(),
                        member_ptr.as_pointer_value().into(),
                        nargs_i32.into(),
                    ];
                    for arg in args {
                        call_args.push(self.compile_expr(arg)?.into());
                    }
                    return Ok(self
                        .builder
                        .build_call(self.rt.cool_module_call, &call_args, "module_call")
                        .unwrap()
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_struct_value());
                }
                if let Some(module_path) = self.imported_user_modules.get(module_name).cloned() {
                    if let Some(module_info) = self.compiled_modules.get(&module_path).cloned() {
                        if let Some(&fn_val) = module_info.functions.get(member) {
                            let params = module_info.function_params.get(member).cloned().unwrap_or_default();
                            let compiled = self.bind_call_args(&params, args, kwargs, 0)?;
                            return Ok(self.call_fn_with_struct_args(fn_val, &compiled, "module_fn_call"));
                        }
                        if let Some(class_info) = module_info.classes.get(member) {
                            return self.call_constructor(
                                class_info.constructor,
                                &class_info.constructor_params,
                                args,
                                kwargs,
                            );
                        }
                    }
                }
            }
        }

        // Handle method calls: obj.method(args)
        if let Expr::Attr {
            object,
            name: method_name,
        } = callee
        {
            if let Expr::Call {
                callee,
                args: super_args,
                kwargs: super_kwargs,
            } = object.as_ref()
            {
                if matches!(callee.as_ref(), Expr::Ident(name) if name == "super")
                    && super_args.is_empty()
                    && super_kwargs.is_empty()
                {
                    let current_class = self.current_class.clone().ok_or("super() used outside method")?;
                    let parent_name = self
                        .classes
                        .get(&current_class)
                        .and_then(|c| c.parent.clone())
                        .ok_or("super(): class has no parent")?;
                    let parent_info = self
                        .classes
                        .get(&parent_name)
                        .ok_or("super(): missing parent metadata")?;
                    let parent_method = *parent_info
                        .methods
                        .get(method_name)
                        .ok_or_else(|| format!("super(): parent has no method '{method_name}'"))?;
                    let self_ptr = self
                        .locals
                        .get("self")
                        .copied()
                        .ok_or("super() called outside of a method")?;
                    let self_val = self
                        .builder
                        .build_load(self.cv_type, self_ptr, "super_self")
                        .unwrap()
                        .into_struct_value();
                    let mut call_args = vec![self_val];
                    for arg in args {
                        call_args.push(self.compile_expr(arg)?);
                    }
                    return Ok(self.call_fn_with_struct_args(parent_method, &call_args, "super_call"));
                }
            }

            let obj_val = self.compile_expr(object)?;
            let attr_name = format!("method_{}", method_name);
            let attr_name_ptr = self.builder.build_global_string_ptr(&attr_name, &attr_name).unwrap();

            // Call method - the runtime looks up the method from the class structure
            let i32t = self.context.i32_type();
            let nargs_i32 = i32t.const_int(args.len() as u64, false); // number of args (excluding self, added by runtime)
            let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![
                obj_val.into(),
                attr_name_ptr.as_pointer_value().into(),
                nargs_i32.into(),
            ];
            for arg in args {
                call_args.push(self.compile_expr(arg)?.into());
            }

            return Ok(self
                .builder
                .build_call(self.rt.cool_call_method_vararg, &call_args, "call_method")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value());
        }

        if let Expr::Ident(name) = callee {
            if self.classes.contains_key(name) {
                let (constructor, ctor_params) = {
                    let class_info = self.classes.get(name).unwrap();
                    (class_info.constructor, class_info.constructor_params.clone())
                };
                return self.call_constructor(constructor, &ctor_params, args, kwargs);
            }
        }

        // Handle closure calls: closure_val(args)
        // This handles any non-Ident callee that might be a closure
        let closure_val = match callee {
            Expr::Ident(n) => {
                // If it's a known function, call it directly
                if let Some(&fn_val) = self.functions.get(n) {
                    let params = self.function_params.get(n).cloned().unwrap_or_default();
                    let compiled = self.bind_call_args(&params, args, kwargs, 0)?;
                    return Ok(self.call_fn_with_struct_args(fn_val, &compiled, "call"));
                }
                // Otherwise, load the variable (might be a closure stored in a local)
                self.locals.get(n).copied().map(|ptr| {
                    self.builder
                        .build_load(self.cv_type, ptr, n)
                        .unwrap()
                        .into_struct_value()
                })
            }
            Expr::Attr { object: _, name: _ } => {
                // Method calls are handled above, but for closures we might get here
                None
            }
            _ => {
                // For other expressions (like nested lambdas), compile and load the result
                Some(self.compile_expr(callee)?)
            }
        };

        // If we have a closure value, call it via the runtime
        if let Some(cv) = closure_val {
            let is_closure = self
                .builder
                .build_call(self.rt.cool_is_closure, &[cv.into()], "is_closure")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();
            let is_ffi = self
                .builder
                .build_call(self.rt.cool_is_ffi_func, &[cv.into()], "is_ffi_func")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

            let fn_val = self.current_fn.unwrap();
            let ffi_check_bb = self.context.append_basic_block(fn_val, "ffi_check");
            let direct_call_bb = self.context.append_basic_block(fn_val, "direct_call");
            let ffi_call_bb = self.context.append_basic_block(fn_val, "ffi_call");
            let closure_call_bb = self.context.append_basic_block(fn_val, "closure_call");
            let after_bb = self.context.append_basic_block(fn_val, "call_after");

            let zero = self.context.i32_type().const_int(0, false);
            let closure_check = self
                .builder
                .build_int_compare(IntPredicate::NE, is_closure, zero, "is_closure_nonzero")
                .unwrap();
            self.builder
                .build_conditional_branch(closure_check, closure_call_bb, ffi_check_bb)
                .unwrap();

            self.builder.position_at_end(ffi_check_bb);
            let ffi_check = self
                .builder
                .build_int_compare(IntPredicate::NE, is_ffi, zero, "is_ffi_nonzero")
                .unwrap();
            self.builder
                .build_conditional_branch(ffi_check, ffi_call_bb, direct_call_bb)
                .unwrap();

            // Direct call path (for regular function values stored in locals)
            self.builder.position_at_end(direct_call_bb);
            // For direct call, we need to look up the function by name or call directly
            let direct_result = if let Expr::Ident(name) = callee {
                if let Some(&fn_val) = self.functions.get(name) {
                    let params = self.function_params.get(name).cloned().unwrap_or_default();
                    let compiled = self.bind_call_args(&params, args, kwargs, 0)?;
                    self.call_fn_with_struct_args(fn_val, &compiled, "direct_call")
                } else {
                    self.builder
                        .build_call(self.rt.cool_noncallable, &[cv.into()], "noncallable")
                        .unwrap()
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_struct_value()
                }
            } else {
                self.builder
                    .build_call(self.rt.cool_noncallable, &[cv.into()], "noncallable")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value()
            };
            let direct_end = self.builder.get_insert_block().unwrap();
            self.builder.build_unconditional_branch(after_bb).unwrap();

            self.builder.position_at_end(ffi_call_bb);
            let ffi_result = self.call_ffi_value(cv, args, kwargs, "call_ffi_value")?;
            let ffi_end = self.builder.get_insert_block().unwrap();
            self.builder.build_unconditional_branch(after_bb).unwrap();

            // Closure call path
            self.builder.position_at_end(closure_call_bb);
            // Store args to global buffer
            let i32t = self.context.i32_type();
            for (i, arg) in args.iter().enumerate() {
                let cv = self.compile_expr(arg)?;
                let idx_val = i32t.const_int(i as u64, false);
                self.builder
                    .build_call(self.rt.cool_set_global_arg, &[idx_val.into(), cv.into()], "set_arg")
                    .unwrap();
            }

            // Get function pointer from closure
            let fn_ptr = self
                .builder
                .build_call(self.rt.cool_closure_get_fn_ptr, &[cv.into()], "get_fn_ptr")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

            // Call the function pointer with nargs
            let nargs_i32 = i32t.const_int(args.len() as u64, false);
            let mut closure_call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![fn_ptr.into(), nargs_i32.into()];
            for arg in args {
                closure_call_args.push(self.compile_expr(arg)?.into());
            }
            let closure_result = self
                .builder
                .build_call(self.rt.cool_call_fn_ptr, &closure_call_args, "call_closure")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value();

            // Merge results
            self.builder.build_unconditional_branch(after_bb).unwrap();
            self.builder.position_at_end(after_bb);
            let phi = self.builder.build_phi(self.cv_type, "call_result").unwrap();
            phi.add_incoming(&[
                (&direct_result, direct_end),
                (&ffi_result, ffi_end),
                (&closure_result, closure_call_bb),
            ]);
            return Ok(phi.as_basic_value().into_struct_value());
        }

        // Fall back to named function call
        let name = match callee {
            Expr::Ident(n) => n.clone(),
            other => return Err(format!("only named function calls are supported; got {other:?}")),
        };

        // ── Check for class instantiation ───────────────────────────────
        if self.classes.contains_key(&name) {
            let (constructor, ctor_params) = {
                let class_info = self.classes.get(&name).unwrap();
                (class_info.constructor, class_info.constructor_params.clone())
            };
            return self.call_constructor(constructor, &ctor_params, args, kwargs);
        }

        // ── asm("template" [, "constraints" [, args...]]) ──
        if name == "asm" {
            if args.is_empty() {
                return Err("asm() requires at least one argument (assembly template string)".into());
            }
            let template = match &args[0] {
                Expr::Str(s) => s.clone(),
                _ => return Err("asm() first argument must be a string literal".into()),
            };
            let (constraints, operand_start) = if args.len() > 1 {
                match &args[1] {
                    Expr::Str(s) => (s.clone(), 2),
                    _ => return Err("asm() second argument must be a string literal (constraints)".into()),
                }
            } else {
                (String::new(), 1)
            };
            // Compile any extra operand args (only present when constraints were given)
            let operands: Vec<BasicMetadataValueEnum<'ctx>> = args[operand_start..]
                .iter()
                .map(|a| self.compile_expr(a).map(|v| v.into()))
                .collect::<Result<_, _>>()?;
            let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> =
                operands.iter().map(|_| self.cv_type.into()).collect();
            let void_fn_type = self.context.void_type().fn_type(&param_types, false);
            let asm_ptr = self.context.create_inline_asm(
                void_fn_type,
                template,
                constraints,
                true,
                false,
                Some(InlineAsmDialect::ATT),
                false,
            );
            self.builder
                .build_indirect_call(void_fn_type, asm_ptr, &operands, "asm")
                .unwrap();
            return Ok(self.build_nil());
        }

        // ── print(...) ──
        if name == "print" {
            let n = args.len() as u64;
            let n_v = self.context.i32_type().const_int(n, false);
            let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![n_v.into()];
            for arg in args {
                let cv = self.compile_expr(arg)?;
                call_args.push(cv.into());
            }
            self.builder.build_call(self.rt.cool_print, &call_args, "").unwrap();
            return Ok(self.build_nil());
        }

        if name == "open" {
            if !kwargs.is_empty() {
                return Err("open() keyword arguments are not yet supported in LLVM backend".into());
            }
            if args.is_empty() || args.len() > 2 {
                return Err("open() takes 1 or 2 arguments".into());
            }
            let path = self.compile_expr(&args[0])?;
            let mode = if args.len() == 2 {
                self.compile_expr(&args[1])?
            } else {
                self.build_str("r")
            };
            return Ok(self
                .builder
                .build_call(self.rt.cool_file_open, &[path.into(), mode.into()], "open")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value());
        }

        // ── raw memory builtins ──
        {
            let unary_mem_fn = match name.as_str() {
                "malloc" => Some(self.rt.cool_malloc),
                "free" => Some(self.rt.cool_free),
                "read_byte" => Some(self.rt.cool_read_byte),
                "read_i64" => Some(self.rt.cool_read_i64),
                "read_f64" => Some(self.rt.cool_read_f64),
                "read_str" => Some(self.rt.cool_read_str),
                _ => None,
            };
            if let Some(fn_val) = unary_mem_fn {
                if args.len() != 1 {
                    return Err(format!("{name}() takes exactly 1 argument"));
                }
                let a = self.compile_expr(&args[0])?;
                return Ok(self.call_unop_fn(fn_val, a, &name));
            }
            let binary_mem_fn = match name.as_str() {
                "write_byte" => Some(self.rt.cool_write_byte),
                "write_i64" => Some(self.rt.cool_write_i64),
                "write_f64" => Some(self.rt.cool_write_f64),
                "write_str" => Some(self.rt.cool_write_str),
                _ => None,
            };
            if let Some(fn_val) = binary_mem_fn {
                if args.len() != 2 {
                    return Err(format!("{name}() takes exactly 2 arguments"));
                }
                let a = self.compile_expr(&args[0])?;
                let b = self.compile_expr(&args[1])?;
                return Ok(self.call_binop_fn(fn_val, a, b, &name));
            }
        }

        // ── range(start, stop, step=1) ────────────────────────────────────────────
        if name == "range" {
            let n = args.len();
            if n < 1 || n > 3 {
                return Err("range() takes 1, 2 or 3 arguments".into());
            }
            let (start, stop, step) = match n {
                1 => {
                    let stop = self.compile_expr(&args[0])?;
                    let zero = self.build_int(0);
                    let one = self.build_int(1);
                    (zero, stop, one)
                }
                2 => {
                    let start = self.compile_expr(&args[0])?;
                    let stop = self.compile_expr(&args[1])?;
                    let one = self.build_int(1);
                    (start, stop, one)
                }
                3 => {
                    let start = self.compile_expr(&args[0])?;
                    let stop = self.compile_expr(&args[1])?;
                    let step = self.compile_expr(&args[2])?;
                    (start, stop, step)
                }
                _ => unreachable!(),
            };
            return Ok(self
                .builder
                .build_call(self.rt.cool_range, &[start.into(), stop.into(), step.into()], "range")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value());
        }

        // ── len(obj) ────────────────────────────────────────────────────────
        if name == "len" {
            if args.len() != 1 {
                return Err("len() takes exactly 1 argument".into());
            }
            let a = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_len, a, "len"));
        }

        // ── type(obj) ────────────────────────────────────────────────────────
        if name == "type" {
            if args.len() != 1 {
                return Err("type() takes exactly 1 argument".into());
            }
            let a = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_type, a, "type"));
        }

        if name == "str" {
            if args.len() != 1 {
                return Err("str() takes exactly 1 argument".into());
            }
            let value = self.compile_expr(&args[0])?;
            let ptr = self
                .builder
                .build_call(self.rt.cool_to_str, &[value.into()], "to_str_builtin")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_pointer_value();
            return Ok(self
                .builder
                .build_call(self.rt.cv_str, &[ptr.into()], "cv_str_builtin")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value());
        }

        if name == "int" {
            if args.len() != 1 {
                return Err("int() takes exactly 1 argument".into());
            }
            let value = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_to_int, value, "int"));
        }

        if name == "float" {
            if args.len() != 1 {
                return Err("float() takes exactly 1 argument".into());
            }
            let value = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_to_float_val, value, "float"));
        }

        if name == "bool" {
            if args.len() != 1 {
                return Err("bool() takes exactly 1 argument".into());
            }
            let value = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_to_bool_val, value, "bool"));
        }

        if name == "round" {
            if args.is_empty() || args.len() > 2 {
                return Err("round() takes 1 or 2 arguments".into());
            }
            let value = self.compile_expr(&args[0])?;
            let digits = if args.len() == 2 {
                self.compile_expr(&args[1])?
            } else {
                self.build_nil()
            };
            return Ok(self.call_binop_fn(self.rt.cool_round, value, digits, "round"));
        }

        if name == "sorted" {
            if args.len() != 1 {
                return Err("sorted() takes exactly 1 argument".into());
            }
            let iterable = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_sorted, iterable, "sorted"));
        }

        if name == "sum" {
            if args.len() != 1 {
                return Err("sum() takes exactly 1 argument".into());
            }
            let iterable = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_sum, iterable, "sum"));
        }

        if name == "abs" {
            if args.len() != 1 {
                return Err("abs() takes exactly 1 argument".into());
            }
            let value = self.compile_expr(&args[0])?;
            return Ok(self.call_unop_fn(self.rt.cool_abs, value, "abs"));
        }

        if name == "min" || name == "max" {
            if args.is_empty() {
                return Err(format!("{name}() requires at least 1 argument"));
            }
            let mut best = self.compile_expr(&args[0])?;
            for arg in &args[1..] {
                let next = self.compile_expr(arg)?;
                let cmp = if name == "min" {
                    self.call_binop_fn(self.rt.cool_lt, next, best, "min_cmp")
                } else {
                    self.call_binop_fn(self.rt.cool_gt, next, best, "max_cmp")
                };
                let fn_val = self.current_fn.unwrap();
                let take_bb = self.context.append_basic_block(fn_val, "minmax_take");
                let keep_bb = self.context.append_basic_block(fn_val, "minmax_keep");
                let done_bb = self.context.append_basic_block(fn_val, "minmax_done");
                let cond_i1 = self.truthy_i1(cmp);
                self.builder
                    .build_conditional_branch(cond_i1, take_bb, keep_bb)
                    .unwrap();

                self.builder.position_at_end(take_bb);
                let take_end = self.builder.get_insert_block().unwrap();
                self.builder.build_unconditional_branch(done_bb).unwrap();

                self.builder.position_at_end(keep_bb);
                let keep_end = self.builder.get_insert_block().unwrap();
                self.builder.build_unconditional_branch(done_bb).unwrap();

                self.builder.position_at_end(done_bb);
                let phi = self.builder.build_phi(self.cv_type, "minmax_phi").unwrap();
                phi.add_incoming(&[(&next, take_end), (&best, keep_end)]);
                best = phi.as_basic_value().into_struct_value();
            }
            return Ok(best);
        }

        if name == "isinstance" {
            if args.len() != 2 {
                return Err("isinstance() takes exactly 2 arguments".into());
            }
            let obj = self.compile_expr(&args[0])?;
            let class_name = match &args[1] {
                Expr::Str(s) => s.clone(),
                _ => return Err("isinstance() currently requires a string literal class name".into()),
            };
            let class_name_ptr = self
                .builder
                .build_global_string_ptr(&class_name, &format!("isinstance_{}", self.str_counter))
                .unwrap();
            self.str_counter += 1;
            return Ok(self
                .builder
                .build_call(
                    self.rt.cool_is_instance,
                    &[obj.into(), class_name_ptr.as_pointer_value().into()],
                    "isinstance",
                )
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value());
        }

        // ── user-defined function ──
        let fn_val = self
            .functions
            .get(&name)
            .copied()
            .ok_or_else(|| format!("undefined function '{name}'"))?;
        let params = self.function_params.get(&name).cloned().unwrap_or_default();
        let compiled = self.bind_call_args(&params, args, kwargs, 0)?;
        Ok(self.call_fn_with_struct_args(fn_val, &compiled, "call"))
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

fn c_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_ascii() => out.push(c),
            c => out.push_str(&format!("\\x{:02x}", c as u32)),
        }
    }
    out
}

pub fn compile_program(program: &Program, output_path: &Path, script_path: &Path) -> Result<(), String> {
    // Initialise LLVM for the host machine
    Target::initialize_native(&InitializationConfig::default()).map_err(|e| format!("LLVM init error: {e}"))?;

    let context = Context::create();
    let mut compiler = Compiler::new(&context);
    compiler.current_source_dir = script_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // ── Pass 1: forward-declare all top-level functions ──────────────────────
    compiler.declare_top_level_functions(program)?;

    // ── Pass 2: build main() and compile all top-level statements ────────────
    let i32_type = context.i32_type();
    let main_fn = compiler.module.add_function("main", i32_type.fn_type(&[], false), None);
    let entry = context.append_basic_block(main_fn, "entry");
    compiler.builder.position_at_end(entry);
    compiler.current_fn = Some(main_fn);
    compiler.allow_toplevel_defs = true;

    compiler.compile_stmts(program)?;

    // Ensure main is properly terminated
    if !compiler.current_block_terminated() {
        let zero = i32_type.const_int(0, false);
        compiler.builder.build_return(Some(&zero)).unwrap();
    }

    compiler
        .module
        .verify()
        .map_err(|e| format!("LLVM module verification failed: {e}"))?;

    // ── Emit LLVM module → object file ───────────────────────────────────────
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
        .write_to_file(&compiler.module, FileType::Object, &obj_path)
        .map_err(|e| format!("Write object file error: {e}"))?;

    // ── Compile C runtime ─────────────────────────────────────────────────────
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();
    let rt_c_path = tmp_dir.join(format!("cool_runtime_{pid}_{nonce}.c"));
    let rt_o_path = tmp_dir.join(format!("cool_runtime_{pid}_{nonce}.o"));

    let runtime_source = format!(
        "static const char* COOL_SCRIPT_PATH = \"{}\";\n{}",
        c_string_literal(&script_path.to_string_lossy()),
        RUNTIME_C
    );
    std::fs::write(&rt_c_path, runtime_source).map_err(|e| format!("Failed to write runtime source: {e}"))?;

    let cc_status = std::process::Command::new("cc")
        .args([
            "-O2",
            "-c",
            rt_c_path.to_str().unwrap(),
            "-o",
            rt_o_path.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("Failed to invoke cc for runtime: {e}"))?;

    if !cc_status.success() {
        return Err("Failed to compile Cool runtime (cc exited with error)".into());
    }

    // ── Link ──────────────────────────────────────────────────────────────────
    let mut link_cmd = std::process::Command::new("cc");
    link_cmd
        .arg(&rt_o_path)
        .arg(&obj_path)
        .arg("-o")
        .arg(output_path)
        .arg("-lm");
    #[cfg(target_os = "linux")]
    link_cmd.arg("-ldl");
    let link_status = link_cmd.status().map_err(|e| format!("Linker error: {e}"))?;

    if !link_status.success() {
        return Err("Linking failed".into());
    }

    // ── Clean up temp files ───────────────────────────────────────────────────
    std::fs::remove_file(&obj_path).ok();
    std::fs::remove_file(&rt_c_path).ok();
    std::fs::remove_file(&rt_o_path).ok();

    Ok(())
}
