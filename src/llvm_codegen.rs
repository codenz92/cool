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
use std::collections::HashMap;
use std::path::Path;

// ── Embedded C runtime ────────────────────────────────────────────────────────

const RUNTIME_C: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <ctype.h>

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
typedef struct {
    int32_t tag;       /* TAG_CLASS */
    int64_t name;      /* const char* */
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
CoolVal cool_index(CoolVal, CoolVal);
CoolVal cool_slice(CoolVal, CoolVal, CoolVal);
CoolVal cool_setindex(CoolVal, CoolVal, CoolVal);
CoolVal cool_round(CoolVal, CoolVal);
CoolVal cool_sorted(CoolVal);
CoolVal cool_sum(CoolVal);
void cool_print(int32_t, ...);

/* ── class / object support ─────────────────────────────────────────── */
CoolVal cool_class_new(const char*, int64_t, int64_t*);
CoolVal cool_object_new(CoolVal);
CoolVal cool_get_attr(CoolVal, const char*);
CoolVal cool_set_attr(CoolVal, const char*, CoolVal);
CoolVal cool_call_method_vararg(CoolVal, const char*, int32_t, ...);
CoolVal cool_get_arg(int32_t);
CoolVal cool_is_instance(CoolVal, const char*);
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
CoolVal cool_class_new(const char* name, int64_t method_count, int64_t* method_ptrs) {
    CoolClass* cls = (CoolClass*)malloc(sizeof(CoolClass) + 2 * method_count * sizeof(int64_t));
    if (!cls) return cv_nil();
    cls->tag = TAG_CLASS;
    cls->name = (int64_t)(intptr_t)name;
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
    if (obj.tag != TAG_OBJECT) return cv_nil();
    CoolObject* o = (CoolObject*)(intptr_t)obj.payload;
    if (!o->attrs) return cv_nil();
    return attrmap_get(o->attrs, name);
}

CoolVal cool_set_attr(CoolVal obj, const char* name, CoolVal value) {
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
        if (strcmp(builtin_name, "join") == 0 && nargs == 1) return cool_string_join(obj, g_method_args[1]);
    }

    if (obj.tag == TAG_LIST && strcmp(builtin_name, "append") == 0 && nargs == 1) {
        cool_list_push(obj, g_method_args[1]);
        return cv_nil();
    }
    if (obj.tag == TAG_DICT && (strcmp(builtin_name, "contains") == 0 || strcmp(builtin_name, "has_key") == 0) && nargs == 1) {
        return cool_dict_contains(obj, g_method_args[1]);
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
    return cv_bool(strcmp((const char*)(intptr_t)o->class->name, class_name) == 0);
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

typedef struct {
    jmp_buf buf;
    int active;
} ExceptionFrame;
static ExceptionFrame g_exception_frames[MAX_EXCEPTION_FRAMES];
static int g_exception_frame_count = 0;
static CoolVal g_current_exception;

/* Set up an exception frame, returns 0 if first time, 1 if longjmp occurred */
int32_t cool_enter_try(void) {
    if (g_exception_frame_count < MAX_EXCEPTION_FRAMES) {
        int idx = g_exception_frame_count;
        g_exception_frames[idx].active = 1;
        int result = setjmp(g_exception_frames[idx].buf);
        g_exception_frame_count++;
        if (result == 0) {
            return 0;  /* normal execution */
        } else {
            return 1;  /* exception caught via longjmp */
        }
    }
    fprintf(stderr, "RuntimeError: too many nested try blocks\n");
    exit(1);
    return 0;
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
            g_exception_frames[i].active = 0;
            longjmp(g_exception_frames[i].buf, 1);
        }
    }
    /* No try frame found - print and exit */
    char* msg = cool_to_str(exc);
    fprintf(stderr, "Unhandled exception: %s\n", msg);
    exit(1);
}

/* Get the current exception value */
CoolVal cool_get_exception(void) {
    return g_current_exception;
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
    cool_contains: FunctionValue<'ctx>,
    // dict operations
    cool_dict_new: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_dict_len: FunctionValue<'ctx>,
    cool_index: FunctionValue<'ctx>,
    cool_slice: FunctionValue<'ctx>,
    cool_setindex: FunctionValue<'ctx>,
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
    cool_enter_try: FunctionValue<'ctx>,
    cool_exit_try: FunctionValue<'ctx>,
    cool_raise: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_get_exception: FunctionValue<'ctx>,
    // module/import
    #[allow(dead_code)]
    cool_get_module: FunctionValue<'ctx>,
    #[allow(dead_code)]
    cool_module_exists: FunctionValue<'ctx>,
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
    loop_stack: Vec<(BasicBlock<'ctx>, BasicBlock<'ctx>)>,
    /// The function currently being compiled (Some(main_fn) at top level).
    current_fn: Option<FunctionValue<'ctx>>,
    /// Captured variables for closures (var name → capture index).
    #[allow(dead_code)]
    captured_vars: HashMap<String, usize>,
    /// All nested function definitions (for closure support).
    nested_functions: Vec<(String, Vec<crate::ast::Param>, Vec<crate::ast::Stmt>)>,
    /// Class currently being compiled, if any.
    current_class: Option<String>,
}

/// Information about a compiled class
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
            cool_class_new: decl!("cool_class_new", cv_type.fn_type(&[ptrm, i64m, ptrm], false)),
            cool_object_new: decl!("cool_object_new", cv_type.fn_type(&[cv], false)),
            cool_get_attr: decl!("cool_get_attr", cv_type.fn_type(&[cv, ptrm], false)),
            cool_set_attr: decl!("cool_set_attr", cv_type.fn_type(&[cv, ptrm, cv], false)),
            cool_call_method_vararg: decl!("cool_call_method_vararg", cv_type.fn_type(&[cv, ptrm, i32m], true)),
            cool_get_arg: decl!("cool_get_arg", cv_type.fn_type(&[i32m], false)),
            cool_set_global_arg: decl!("cool_set_global_arg", voidt.fn_type(&[i32m, cv], false)),
            cool_is_instance: decl!("cool_is_instance", cv_type.fn_type(&[cv, ptrm], false)),
            cool_contains: decl!("cool_contains", cv_type.fn_type(&[cv, cv], false)),
            // dict operations
            cool_dict_new: decl!("cool_dict_new", cv_type.fn_type(&[], false)),
            cool_dict_len: decl!("cool_dict_len", cv_type.fn_type(&[cv], false)),
            cool_index: decl!("cool_index", cv_type.fn_type(&[cv, cv], false)),
            cool_slice: decl!("cool_slice", cv_type.fn_type(&[cv, cv, cv], false)),
            cool_setindex: decl!("cool_setindex", cv_type.fn_type(&[cv, cv, cv], false)),
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
            cool_enter_try: decl!("cool_enter_try", i32t.fn_type(&[], false)),
            cool_exit_try: decl!("cool_exit_try", voidt.fn_type(&[], false)),
            cool_raise: decl!("cool_raise", voidt.fn_type(&[cv], false)),
            cool_get_exception: decl!("cool_get_exception", cv_type.fn_type(&[], false)),
            // module/import
            cool_get_module: decl!("cool_get_module", cv_type.fn_type(&[ptrm], false)),
            cool_module_exists: decl!("cool_module_exists", i32t.fn_type(&[ptrm], false)),
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

    fn is_main(&self) -> bool {
        self.current_fn
            .map(|f| f.get_name().to_str().unwrap_or("") == "main")
            .unwrap_or(false)
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
                if self.is_main() {
                    // top-level return → exit normally
                    if let Some(e) = opt_expr {
                        self.compile_expr(e)?; // side-effects only
                    }
                    let zero = self.context.i32_type().const_int(0, false);
                    self.builder.build_return(Some(&zero)).unwrap();
                } else {
                    let val = match opt_expr {
                        Some(e) => self.compile_expr(e)?,
                        None => self.build_nil(),
                    };
                    self.builder.build_return(Some(&val)).unwrap();
                }
            }

            // ── break / continue ─────────────────────────────────────────────
            Stmt::Break => {
                let (_, break_bb) = *self.loop_stack.last().ok_or("'break' used outside loop")?;
                self.builder.build_unconditional_branch(break_bb).unwrap();
            }
            Stmt::Continue => {
                let (cont_bb, _) = *self.loop_stack.last().ok_or("'continue' used outside loop")?;
                self.builder.build_unconditional_branch(cont_bb).unwrap();
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
                    let seq_cur = self.builder.build_load(self.cv_type, seq_ptr, "unpack_seq").unwrap().into_struct_value();
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
            Stmt::Try { body, handlers, else_body, finally_body } => {
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
        self.loop_stack.push((cond_bb, after_bb));
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
        let body_bb = self.context.append_basic_block(fn_val, "for_body");
        let after_bb = self.context.append_basic_block(fn_val, "for_after");
        let update_bb = self.context.append_basic_block(fn_val, "for_update");

        // Compile the iterable into an index variable
        let iter_val = self.compile_expr(iter)?;
        let idx_ptr = self.builder.build_alloca(self.cv_type, "for_idx").unwrap();
        let zero = self.build_int(0);
        self.builder.build_store(idx_ptr, zero).unwrap();

        // Allocate the loop variable
        let var_ptr = self.build_entry_alloca(var);
        self.locals.insert(var.to_string(), var_ptr);

        // Get length of list (computed but not needed at runtime yet)
        let _len_for_unused = self.call_unop_fn(self.rt.cool_list_len, iter_val.clone(), "len");

        // Jump to condition check
        self.builder.build_unconditional_branch(update_bb).unwrap();

        // Update: check idx < len
        self.builder.position_at_end(update_bb);
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
        self.loop_stack.push((update_bb, after_bb));
        let elem = self.call_binop_fn(self.rt.cool_list_get, iter_val.clone(), idx_cv, "get");
        self.builder.build_store(var_ptr, elem).unwrap();
        self.compile_stmts(body)?;
        if !self.current_block_terminated() {
            // Increment index
            let one = self.build_int(1);
            let old_idx = self
                .builder
                .build_load(self.cv_type, idx_ptr, "old_idx")
                .unwrap()
                .into_struct_value();
            let new_idx = self.call_binop_fn(self.rt.cool_add, old_idx, one, "add");
            self.builder.build_store(idx_ptr, new_idx).unwrap();
            self.builder.build_unconditional_branch(update_bb).unwrap();
        }
        self.loop_stack.pop();

        self.builder.position_at_end(after_bb);
        Ok(())
    }

    // ── function definition ───────────────────────────────────────────────────

    fn compile_fndef(&mut self, name: &str, params: &[crate::ast::Param], body: &[Stmt]) -> Result<(), String> {
        if !self.is_main() {
            return Err("nested function definitions are not supported in the LLVM backend".into());
        }

        let fn_val = *self
            .functions
            .get(name)
            .ok_or_else(|| format!("function '{name}' was not pre-declared"))?;

        // Save caller state
        let saved_bb = self.builder.get_insert_block();
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_fn = self.current_fn.replace(fn_val);
        let saved_loops = std::mem::take(&mut self.loop_stack);

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
        self.current_fn = saved_fn;
        self.loop_stack = saved_loops;
        if let Some(bb) = saved_bb {
            self.builder.position_at_end(bb);
        }
        Ok(())
    }

    // ── class definition ─────────────────────────────────────────────────────

    fn compile_class(&mut self, name: &str, parent: Option<&str>, body: &[Stmt]) -> Result<(), String> {
        if !self.is_main() {
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
                    name: mname,
                    params,
                    ..
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
                let fn_name = format!("{}#{}.{}", name, mname, name);
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
                    let saved_fn = self.current_fn.replace(fn_val);
                    let saved_loops = std::mem::take(&mut self.loop_stack);
                    let saved_class = self.current_class.replace(name.to_string());

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
                    self.current_fn = saved_fn;
                    self.loop_stack = saved_loops;
                    self.current_class = saved_class;
                    if let Some(bb) = saved_bb {
                        self.builder.position_at_end(bb);
                    }
                }
            }
        }

        // Build the constructor function
        let ctor_name = format!("{}#constructor.{}", name, name);
        let ctor_type = self.cv_type.fn_type(&[], false);
        let constructor = self.module.add_function(&ctor_name, ctor_type, None);

        // Build constructor body
        let saved_bb = self.builder.get_insert_block();
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_fn = self.current_fn.replace(constructor);
        let saved_loops = std::mem::take(&mut self.loop_stack);

        let entry = self.context.append_basic_block(constructor, "entry");
        self.builder.position_at_end(entry);

        // Build method data array: [name_ptr1, fn_ptr1, name_ptr2, fn_ptr2, ...]
        let method_count = methods.len() as i64;

        // Allocate array for method data (2 i64 values per method: name ptr and fn ptr)
        let method_data_size = method_count * 2 * 8; // 2 * i64 per method
        let method_data_size_val = self.build_int(method_data_size);
        let method_data_ptr = self
            .builder
            .build_call(
                self.rt.cool_malloc,
                &[method_data_size_val.into()],
                "method_data_ptr",
            )
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
        let class_val = self
            .builder
            .build_call(
                self.rt.cool_class_new,
                &[
                    name_ptr.as_pointer_value().into(),
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
        let global_name = format!("__class_{}", name);
        let _global = self.module.add_global(self.cv_type, None, &global_name);

        // At runtime, we need to initialize this - for now, just store constructor ref
        let _constructor_holder = self
            .builder
            .build_alloca(self.cv_type, &format!("{}_holder", name))
            .unwrap();

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

    // ── try / except / else / finally ────────────────────────────────────────
    //
    // Uses setjmp/longjmp for exception handling. cool_enter_try() returns 0 on
    // normal entry, 1 on longjmp (exception caught). cool_exit_try() cleans up.
    // cool_raise() transfers control to the nearest try frame.
    //
    fn compile_try(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        else_body: Option<&[Stmt]>,
        finally_body: Option<&[Stmt]>,
    ) -> Result<(), String> {
        let fn_val = self.current_fn.unwrap();

        // Call cool_enter_try() which does setjmp
        let result = self
            .builder
            .build_call(self.rt.cool_enter_try, &[], "enter_try")
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
        let handler_bb = self.context.append_basic_block(fn_val, "exception_handler");
        let merge_bb = self.context.append_basic_block(fn_val, "try_merge");

        self.builder
            .build_conditional_branch(caught_i1, handler_bb, try_bb)
            .unwrap();

        // ── Normal path: execute try body ──────────────────────────────────
        self.builder.position_at_end(try_bb);
        self.compile_stmts(body)?;

        // Run else body if no exception occurred
        let _else_bb = if else_body.is_some() {
            let bb = self.context.append_basic_block(fn_val, "else_body");
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(bb).unwrap();
            }
            self.builder.position_at_end(bb);
            if let Some(stmts) = else_body {
                self.compile_stmts(stmts)?;
            }
            bb
        } else {
            try_bb
        };

        if !self.current_block_terminated() {
            self.builder.build_unconditional_branch(merge_bb).unwrap();
        }

        // ── Exception handler path ──────────────────────────────────────────
        self.builder.position_at_end(handler_bb);
        // Get the exception value from cool_get_exception()
        let exc_val = self
            .builder
            .build_call(self.rt.cool_get_exception, &[], "get_exc")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();

        let mut handled = false;
        for handler in handlers {
            let matches = handler.exc_type.is_none()
                || {
                    // For type-based matching, we just match bare except or type names
                    // In practice, we handle this by storing exception type info
                    // For simplicity, bare except catches all, typed catches need more work
                    true
                };

            if matches {
                // Create handler scope
                let handler_env_bb = self.context.append_basic_block(fn_val, "handler_body");
                self.builder.build_unconditional_branch(handler_env_bb).unwrap();
                self.builder.position_at_end(handler_env_bb);

                // Bind the exception to the 'as' name if present
                if let Some(as_name) = &handler.as_name {
                    let ptr = self.build_entry_alloca(as_name);
                    self.builder.build_store(ptr, exc_val).unwrap();
                    self.locals.insert(as_name.clone(), ptr);
                }

                self.compile_stmts(&handler.body)?;

                if !self.current_block_terminated() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }

                handled = true;
                break;
            }
        }

        if !handled {
            // Re-raise if no handler matched - exit try and re-raise
            self.builder
                .build_call(self.rt.cool_exit_try, &[], "exit_try")
                .unwrap();
            // Call cool_raise again to propagate
            self.builder
                .build_call(self.rt.cool_raise, &[exc_val.into()], "re_raise")
                .unwrap();
            self.builder.build_unreachable().unwrap();
        }

        // ── Finally block ────────────────────────────────────────────────────
        // Position at merge or finally if present
        if let Some(finally) = finally_body {
            let finally_bb = self.context.append_basic_block(fn_val, "finally_body");
            self.builder.position_at_end(merge_bb);
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(finally_bb).unwrap();
            }
            self.builder.position_at_end(finally_bb);
            self.compile_stmts(finally)?;
            if !self.current_block_terminated() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }
        }

        self.builder.position_at_end(merge_bb);

        // Clean up the try frame
        self.builder
            .build_call(self.rt.cool_exit_try, &[], "exit_try")
            .unwrap();

        Ok(())
    }

    // ── raise ────────────────────────────────────────────────────────────────
    fn compile_raise(&mut self, opt_expr: Option<&Expr>) -> Result<(), String> {
        let exc_val = if let Some(e) = opt_expr {
            self.compile_expr(e)?
        } else {
            self.build_str("Exception")
        };

        // Call cool_raise - this does longjmp if a try frame is active
        self.builder
            .build_call(self.rt.cool_raise, &[exc_val.into()], "raise")
            .unwrap();
        // If longjmp doesn't happen (no try frame), we continue
        self.builder.build_unreachable().unwrap();
        Ok(())
    }

    // ── import "path.cool" ────────────────────────────────────────────────────
    fn compile_import(&mut self, _path: &str) -> Result<(), String> {
        // LLVM backend doesn't support dynamic compilation
        Err("import is not yet supported in LLVM backend (requires dynamic compilation)".into())
    }

    // ── import module_name ────────────────────────────────────────────────────
    fn compile_import_module(&mut self, name: &str) -> Result<(), String> {
        match name {
            "math" | "os" | "sys" | "time" | "random" | "json" | "re" | "string"
            | "list" | "collections" => {
                // Built-in modules are handled via the runtime's module registry
                // Get the module dict from runtime
                let module_name_ptr = self.builder.build_global_string_ptr(name, &format!("mod_{}", name)).unwrap();
                let dict_val = self
                    .builder
                    .build_call(self.rt.cool_get_module, &[module_name_ptr.as_pointer_value().into()], "module_dict")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value();
                // Create a local variable to store the module reference
                let ptr = self.build_entry_alloca(name);
                self.builder.build_store(ptr, dict_val).unwrap();
                self.locals.insert(name.to_string(), ptr);
                Ok(())
            }
            _ => Err(format!(
                "import: unknown module '{}' (only math, os, sys, time, random, json, re, string, list, collections supported in LLVM backend)",
                name
            )),
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
                    let val = self.builder.build_load(self.cv_type, *ptr, "capture_load").unwrap().into_struct_value();
                    let idx_val = i32t.const_int(i as u64, false);
                    self.builder.build_call(
                        self.rt.cool_set_closure_capture,
                        &[idx_val.into(), val.into()],
                        "set_capture",
                    ).unwrap();
                }

                // Get function pointer as i64 using pointer-to-int cast
                let fn_ptr_val = lambda_fn.as_global_value().as_pointer_value();
                let fn_ptr_int = self.builder.build_ptr_to_int(
                    fn_ptr_val,
                    self.context.i64_type(),
                    "fn_ptr_int"
                ).unwrap();
                
                // Create null pointer for captures array (we use global storage instead)
                let null_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
                
                let closure = self.builder
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
                self.nested_functions.push((
                    fn_name.clone(),
                    params.clone(),
                    vec![Stmt::Return(Some(*body.clone()))],
                ));

                // We'll compile nested functions at the end. For now, return the closure.
                // Note: We need to register the function for later compilation.
                // The nested_functions vec handles this.

                Ok(closure)
            }

            Expr::Ternary { condition, then_expr, else_expr } => {
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

            Expr::ListComp { expr, var, iter, condition } => {
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
                let idx_cv = self.builder.build_load(self.cv_type, idx_ptr, "lc_idx_load").unwrap().into_struct_value();
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
                    let old_idx = self.builder.build_load(self.cv_type, idx_ptr, "lc_skip_idx").unwrap().into_struct_value();
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
                    self.builder.build_load(self.cv_type, var_ptr, "lc_var").unwrap().into_struct_value();
                    self.compile_expr(expr)?
                } else {
                    self.compile_expr(expr)?
                };
                let result_cv = self.builder.build_load(self.cv_type, result_ptr, "lc_res_load").unwrap().into_struct_value();
                self.call_binop_fn(self.rt.cool_list_push, result_cv, push_elem, "lc_push");

                // Increment idx
                let old_idx2 = self.builder.build_load(self.cv_type, idx_ptr, "lc_old_idx").unwrap().into_struct_value();
                let one_inc = self.build_int(1);
                let new_idx2 = self.call_binop_fn(self.rt.cool_add, old_idx2, one_inc, "lc_inc2");
                self.builder.build_store(idx_ptr, new_idx2).unwrap();
                self.builder.build_unconditional_branch(cond_bb).unwrap();

                self.builder.position_at_end(after_bb);

                // Restore shadowed variable if any
                match saved_var {
                    Some(ptr) => { self.locals.insert(var.clone(), ptr); }
                    None => { self.locals.remove(var); }
                }

                Ok(self.builder.build_load(self.cv_type, result_ptr, "lc_final").unwrap().into_struct_value())
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
                let dict_val = self.builder
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
                    let cur = self.builder.build_load(self.cv_type, dict_ptr, "dict_cur").unwrap().into_struct_value();
                    let updated = self.call_triop_fn(self.rt.cool_setindex, cur, k_val, v_val, "dict_set");
                    self.builder.build_store(dict_ptr, updated).unwrap();
                }
                Ok(self.builder.build_load(self.cv_type, dict_ptr, "dict_final").unwrap().into_struct_value())
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

    // ── Function call ─────────────────────────────────────────────────────────

    fn compile_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<StructValue<'ctx>, String> {
        // Handle method calls: obj.method(args)
        if let Expr::Attr {
            object,
            name: method_name,
        } = callee
        {
            if let Expr::Call { callee, args: super_args, kwargs: super_kwargs } = object.as_ref() {
                if matches!(callee.as_ref(), Expr::Ident(name) if name == "super") && super_args.is_empty() && super_kwargs.is_empty() {
                    let current_class = self.current_class.clone().ok_or("super() used outside method")?;
                    let parent_name = self
                        .classes
                        .get(&current_class)
                        .and_then(|c| c.parent.clone())
                        .ok_or("super(): class has no parent")?;
                    let parent_info = self.classes.get(&parent_name).ok_or("super(): missing parent metadata")?;
                    let parent_method = *parent_info
                        .methods
                        .get(method_name)
                        .ok_or_else(|| format!("super(): parent has no method '{method_name}'"))?;
                    let self_ptr = self.locals.get("self").copied().ok_or("super() called outside of a method")?;
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
            let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> =
                vec![obj_val.into(), attr_name_ptr.as_pointer_value().into(), nargs_i32.into()];
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
                self.locals.get(n).copied()
                    .map(|ptr| {
                        self.builder.build_load(self.cv_type, ptr, n).unwrap().into_struct_value()
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
            // Check if it's a closure using cool_is_closure
            let is_closure = self.builder
                .build_call(self.rt.cool_is_closure, &[cv.into()], "is_closure")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

            let fn_val = self.current_fn.unwrap();
            let direct_call_bb = self.context.append_basic_block(fn_val, "direct_call");
            let closure_call_bb = self.context.append_basic_block(fn_val, "closure_call");
            let after_bb = self.context.append_basic_block(fn_val, "call_after");

            let zero = self.context.i32_type().const_int(0, false);
            let is_zero = self.builder.build_int_compare(IntPredicate::EQ, is_closure, zero, "is_zero").unwrap();
            self.builder.build_conditional_branch(is_zero, direct_call_bb, closure_call_bb).unwrap();

            // Direct call path (for regular function values stored in locals)
            self.builder.position_at_end(direct_call_bb);
            // For direct call, we need to look up the function by name or call directly
            let direct_result = if let Expr::Ident(name) = callee {
                if let Some(&fn_val) = self.functions.get(name) {
                    let params = self.function_params.get(name).cloned().unwrap_or_default();
                    let compiled = self.bind_call_args(&params, args, kwargs, 0)?;
                    self.call_fn_with_struct_args(fn_val, &compiled, "direct_call")
                } else {
                    return Err(format!("undefined function '{}'", name));
                }
            } else {
                // For non-identifier callees in direct call path, just compile and return nil
                self.build_nil()
            };
            let direct_end = self.builder.get_insert_block().unwrap();
            self.builder.build_unconditional_branch(after_bb).unwrap();

            // Closure call path
            self.builder.position_at_end(closure_call_bb);
            // Store args to global buffer
            let i32t = self.context.i32_type();
            for (i, arg) in args.iter().enumerate() {
                let cv = self.compile_expr(arg)?;
                let idx_val = i32t.const_int(i as u64, false);
                self.builder.build_call(
                    self.rt.cool_set_global_arg,
                    &[idx_val.into(), cv.into()],
                    "set_arg",
                ).unwrap();
            }

            // Get function pointer from closure
            let fn_ptr = self.builder
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
            let closure_result = self.builder
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
            phi.add_incoming(&[(&direct_result, direct_end), (&closure_result, closure_call_bb)]);
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
            let compiled = self.bind_call_args(&ctor_params, args, kwargs, 0)?;

            // Compile arguments and store to global buffer for constructor
            let i32t = self.context.i32_type();
            for (i, cv) in compiled.iter().enumerate() {
                let idx_val = i32t.const_int(i as u64, false);
                self.builder
                    .build_call(self.rt.cool_set_global_arg, &[idx_val.into(), (*cv).into()], "set_global_arg")
                    .unwrap();
            }

            // Call the constructor (which reads args from global buffer)
            return Ok(self
                .builder
                .build_call(constructor, &[], "instantiate")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value());
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

pub fn compile_program(program: &Program, output_path: &Path) -> Result<(), String> {
    // Initialise LLVM for the host machine
    Target::initialize_native(&InitializationConfig::default()).map_err(|e| format!("LLVM init error: {e}"))?;

    let context = Context::create();
    let mut compiler = Compiler::new(&context);

    // ── Pass 1: forward-declare all top-level functions ──────────────────────
    for stmt in program {
        if let Stmt::FnDef { name, params, .. } = stmt {
            if compiler.functions.contains_key(name) {
                continue; // already declared (duplicate def — let later pass error)
            }
            // Check for unsupported params early so we can give a clean error
            if params.iter().any(|p| p.is_vararg || p.is_kwarg) {
                return Err(format!(
                    "function '{name}': *args / **kwargs are not supported in LLVM backend"
                ));
            }
            let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'_>> =
                params.iter().map(|_| compiler.cv_type.into()).collect();
            let fn_type = compiler.cv_type.fn_type(&param_types, false);
            let fn_val = compiler.module.add_function(name, fn_type, None);
            compiler.functions.insert(name.clone(), fn_val);
            compiler.function_params.insert(name.clone(), params.clone());
        }
    }

    // ── Pass 2: build main() and compile all top-level statements ────────────
    let i32_type = context.i32_type();
    let main_fn = compiler.module.add_function("main", i32_type.fn_type(&[], false), None);
    let entry = context.append_basic_block(main_fn, "entry");
    compiler.builder.position_at_end(entry);
    compiler.current_fn = Some(main_fn);

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
    let rt_c_path = std::path::Path::new("/tmp/cool_runtime.c");
    let rt_o_path = std::path::Path::new("/tmp/cool_runtime.o");

    std::fs::write(rt_c_path, RUNTIME_C).map_err(|e| format!("Failed to write runtime source: {e}"))?;

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
    let link_status = std::process::Command::new("cc")
        .arg(rt_o_path)
        .arg(&obj_path)
        .arg("-o")
        .arg(output_path)
        .arg("-lm")
        .status()
        .map_err(|e| format!("Linker error: {e}"))?;

    if !link_status.success() {
        return Err("Linking failed".into());
    }

    // ── Clean up temp files ───────────────────────────────────────────────────
    std::fs::remove_file(&obj_path).ok();
    std::fs::remove_file(rt_o_path).ok();

    Ok(())
}
