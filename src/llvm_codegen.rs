// cool-lang/src/llvm_codegen.rs
//
// LLVM backend for Cool.
//
// Architecture:
//   1. Embedded C runtime (RUNTIME_C const) defines CoolVal and all operations.
//   2. The Compiler emits LLVM IR that calls those C functions.
//   3. compile_program() writes the runtime to /tmp, compiles it with `cc`,
//      emits the LLVM module to a .o file, then links both together.

use crate::ast::{BinOp, Expr, Program, Stmt, UnaryOp};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::StructType;
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, FunctionValue, IntValue, PointerValue, StructValue,
};
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

#define TAG_NIL    0
#define TAG_INT    1
#define TAG_FLOAT  2
#define TAG_BOOL   3
#define TAG_STR    4
#define TAG_LIST   5
#define TAG_OBJECT 6
#define TAG_CLASS  7

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
CoolVal cool_list_make(int64_t);
CoolVal cool_list_len(CoolVal);
CoolVal cool_type(CoolVal);
CoolVal cool_list_get(CoolVal, CoolVal);
CoolVal cool_list_push(CoolVal, CoolVal);
CoolVal cool_list_concat(CoolVal, CoolVal);
void cool_print(int32_t, ...);

/* ── class / object support ─────────────────────────────────────────── */
CoolVal cool_class_new(const char*, int64_t, int64_t*);
CoolVal cool_object_new(CoolVal);
CoolVal cool_get_attr(CoolVal, const char*);
CoolVal cool_set_attr(CoolVal, const char*, CoolVal);
CoolVal cool_call_method_vararg(CoolVal, const char*, int32_t, ...);
CoolVal cool_get_arg(int32_t);
CoolVal cool_is_instance(CoolVal, const char*);

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

/* ── arithmetic ───────────────────────────────────────────────────────── */
CoolVal cool_add(CoolVal a, CoolVal b) {
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
CoolVal cool_list_make(int64_t n) {
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

/* ── to_str ─────────���─────────────────────────────────────────────────── */
char* cool_to_str(CoolVal v) {
    if (v.tag == TAG_STR) return (char*)(intptr_t)v.payload;
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
        case TAG_OBJECT: return "object";
        default:         return "unknown";
    }
}

CoolVal cool_type(CoolVal v) {
    return cv_str(cool_type_name(v.tag));
}

CoolVal cool_list_get(CoolVal list_val, CoolVal idx_val) {
    if (list_val.tag != TAG_LIST) return cv_nil();
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
    if (list_val.tag != TAG_LIST) return cv_nil();
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
        case TAG_LIST: {
            CoolList* lst = (CoolList*)(intptr_t)v.payload;
            return cv_int(lst->length);
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

CoolVal cool_call_method_vararg(CoolVal obj, const char* name, int32_t nargs, ...) {
    if (obj.tag != TAG_OBJECT) return cv_nil();
    CoolObject* o = (CoolObject*)(intptr_t)obj.payload;
    if (!o->class) return cv_nil();
    
    // Look up method from class structure
    int64_t method_ptr = 0;
    CoolClass* cls = o->class;
    
    for (int64_t i = 0; i < cls->method_count; i++) {
        const char* mname = (const char*)(intptr_t)cls->methods[i * 2];
        if (mname && strcmp(mname, name) == 0) {
            method_ptr = cls->methods[i * 2 + 1];
            break;
        }
    }
    
    if (method_ptr == 0) {
        fprintf(stderr, "AttributeError: '%s' object has no attribute '%s'\n",
                (const char*)(intptr_t)o->class->name, name);
        exit(1);
    }
    
    va_list ap;
    va_start(ap, nargs);
    g_method_args[0] = obj;  /* self */
    for (int32_t i = 0; i < nargs && i < 31; i++) {
        g_method_args[i + 1] = va_arg(ap, CoolVal);
    }
    g_method_arg_count = nargs + 1;
    va_end(ap);
    
    typedef CoolVal (*CoolFn)(void);
    CoolFn fn = (CoolFn)(intptr_t)method_ptr;
    return fn();
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
"#;

// ── Runtime function table ────────────────────────────────────────────────────

struct RuntimeFns<'ctx> {
    cv_nil: FunctionValue<'ctx>,
    cv_int: FunctionValue<'ctx>,
    cv_float: FunctionValue<'ctx>,
    cv_bool: FunctionValue<'ctx>,
    cv_str: FunctionValue<'ctx>,
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
    // list operations
    cool_list_make: FunctionValue<'ctx>,
    cool_list_len: FunctionValue<'ctx>,
    cool_list_get: FunctionValue<'ctx>,
    cool_list_set: FunctionValue<'ctx>,
    cool_list_push: FunctionValue<'ctx>,
    cool_list_pop: FunctionValue<'ctx>,
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
    cool_is_instance: FunctionValue<'ctx>,
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
    /// Top-level user-defined classes (name → ClassInfo).
    classes: HashMap<String, ClassInfo<'ctx>>,
    str_counter: usize,
    /// (continue_target, break_target) for each enclosing loop.
    loop_stack: Vec<(BasicBlock<'ctx>, BasicBlock<'ctx>)>,
    /// The function currently being compiled (Some(main_fn) at top level).
    current_fn: Option<FunctionValue<'ctx>>,
}

/// Information about a compiled class
struct ClassInfo<'ctx> {
    /// The class constructor function (returns CoolVal)
    constructor: FunctionValue<'ctx>,
    /// Method names and their function values
    methods: HashMap<String, FunctionValue<'ctx>>,
    /// Attribute default values (compiled)
    attributes: Vec<(String, Expr)>,
}

// ── Constructor & runtime declarations ───────────────────────────────────────

impl<'ctx> Compiler<'ctx> {
    fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("cool_program");
        let builder = context.create_builder();

        // %CoolVal = type { i32, i64 }
        let cv_type = context.opaque_struct_type("CoolVal");
        cv_type.set_body(
            &[context.i32_type().into(), context.i64_type().into()],
            false,
        );

        let rt = Self::declare_runtime(context, &module, cv_type);

        Compiler {
            context,
            module,
            builder,
            cv_type,
            rt,
            locals: HashMap::new(),
            functions: HashMap::new(),
            classes: HashMap::new(),
            str_counter: 0,
            loop_stack: Vec::new(),
            current_fn: None,
        }
    }

    fn declare_runtime(
        context: &'ctx Context,
        module: &Module<'ctx>,
        cv_type: StructType<'ctx>,
    ) -> RuntimeFns<'ctx> {
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
            cool_list_make: decl!("cool_list_make", cv_type.fn_type(&[i64m], false)),
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

    // Call a unary-op runtime function.
    fn call_unop_fn(
        &mut self,
        fn_val: FunctionValue<'ctx>,
        a: StructValue<'ctx>,
        name: &str,
    ) -> StructValue<'ctx> {
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
                return Err("'in'/'not in' not supported in LLVM backend".into());
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
                    let p = self.builder.build_alloca(self.cv_type, name).unwrap();
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
                let attr_name_ptr = self.builder.build_global_string_ptr(name, &format!("attr_{}", name)).unwrap();
                self.builder
                    .build_call(self.rt.cool_set_attr, &[obj_val.into(), attr_name_ptr.as_pointer_value().into(), val.into()], "set_attr")
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
                let (cont_bb, _) = *self
                    .loop_stack
                    .last()
                    .ok_or("'continue' used outside loop")?;
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
        self.builder
            .build_conditional_branch(i1, then_bb, else_entry)
            .unwrap();

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
            self.builder
                .build_conditional_branch(i1, elif_then, elif_else)
                .unwrap();

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
        self.builder
            .build_conditional_branch(i1, body_bb, after_bb)
            .unwrap();

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
        let var_ptr = self.builder.build_alloca(self.cv_type, var).unwrap();
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
        self.builder
            .build_conditional_branch(i1, body_bb, after_bb)
            .unwrap();

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

    fn compile_fndef(
        &mut self,
        name: &str,
        params: &[crate::ast::Param],
        body: &[Stmt],
    ) -> Result<(), String> {
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
                let alloca = self
                    .builder
                    .build_alloca(self.cv_type, &param.name)
                    .unwrap();
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

    fn compile_class(
        &mut self,
        name: &str,
        _parent: Option<&str>,
        body: &[Stmt],
    ) -> Result<(), String> {
        if !self.is_main() {
            return Err("class definitions are only allowed at the top level".into());
        }

        // Collect method names and check for __init__
        let mut methods: HashMap<String, FunctionValue<'ctx>> = HashMap::new();
        let mut has_init = false;
        let mut init_body: Option<Vec<Stmt>> = None;
        let mut init_params: Option<Vec<crate::ast::Param>> = None;
        let mut attributes: Vec<(String, Expr)> = Vec::new();

        for stmt in body {
            match stmt {
                Stmt::FnDef { name: mname, params, body: mbody } => {
                    if mname == "__init__" {
                        has_init = true;
                        init_body = Some(mbody.clone());
                        init_params = Some(params.clone());
                    }
                    // Methods will be compiled after we register the class
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
        let name_ptr = self.builder.build_global_string_ptr(name, &format!("class_name_{}", name)).unwrap();

        // First, declare stub functions for all methods
        for stmt in body {
            if let Stmt::FnDef { name: mname, params, .. } = stmt {
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
            if let Stmt::FnDef { name: mname, params, body: mbody } = stmt {
                if let Some(&fn_val) = methods.get(mname) {
                    // Save state
                    let saved_bb = self.builder.get_insert_block();
                    let saved_locals = std::mem::take(&mut self.locals);
                    let saved_fn = self.current_fn.replace(fn_val);
                    let saved_loops = std::mem::take(&mut self.loop_stack);

                    // Build entry
                    let entry = self.context.append_basic_block(fn_val, "entry");
                    self.builder.position_at_end(entry);

                    // Bind self as first param (or from cool_get_arg(0) for variadic call)
                    let self_ptr = self.builder.build_alloca(self.cv_type, "self").unwrap();
                    let i32t = self.context.i32_type();
                    
                    // Load self from global args buffer
                    let self_val = self.builder
                        .build_call(self.rt.cool_get_arg, &[i32t.const_int(0, false).into()], "get_self")
                        .unwrap()
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_struct_value();
                    self.builder.build_store(self_ptr, self_val).unwrap();
                    self.locals.insert("self".to_string(), self_ptr);

                    // Bind other params
                    for (i, param) in params.iter().enumerate() {
                        if param.is_vararg || param.is_kwarg {
                            return Err("*args / **kwargs not supported in methods".into());
                        }
                        if let Some(param_val) = fn_val.get_nth_param(i as u32) {
                            let alloca = self.builder.build_alloca(self.cv_type, &param.name).unwrap();
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
        let method_data_ptr = self.builder
            .build_call(self.rt.cool_malloc, &[self.context.i64_type().const_int(method_data_size as u64, false).into()], "method_data_ptr")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();
        
        // Get the raw pointer
        let method_data_int = self.builder.build_extract_value(method_data_ptr, 1, "method_data_int").unwrap().into_int_value();
        let method_data_i8ptr = self.builder.build_int_to_ptr(method_data_int, self.context.i8_type().ptr_type(inkwell::AddressSpace::default()), "method_data_i8ptr").unwrap();
        
        // Fill in method data
        for (i, (method_name, &fn_val)) in methods.iter().enumerate() {
            let idx = i as u64;
            
            // Store name pointer at offset idx * 16
            let name_offset = self.context.i64_type().const_int(idx * 16, false);
            let name_ptr_pos = unsafe { self.builder.build_in_bounds_gep(self.context.i8_type(), method_data_i8ptr, &[name_offset], "name_ptr_pos").unwrap() };
            let name_ptr_cast = self.builder.build_pointer_cast(name_ptr_pos, self.context.i64_type().ptr_type(inkwell::AddressSpace::default()), "name_ptr_cast").unwrap();
            let attr_name = format!("method_{}", method_name);
            let method_name_ptr = self.builder.build_global_string_ptr(&attr_name, &attr_name).unwrap();
            let name_as_int = self.builder.build_ptr_to_int(method_name_ptr.as_pointer_value(), self.context.i64_type(), "name_int").unwrap();
            self.builder.build_store(name_ptr_cast, name_as_int).unwrap();
            
            // Store function pointer at offset idx * 16 + 8
            let fn_offset = self.context.i64_type().const_int(idx * 16 + 8, false);
            let fn_ptr_pos = unsafe { self.builder.build_in_bounds_gep(self.context.i8_type(), method_data_i8ptr, &[fn_offset], "fn_ptr_pos").unwrap() };
            let fn_ptr_cast = self.builder.build_pointer_cast(fn_ptr_pos, self.context.i64_type().ptr_type(inkwell::AddressSpace::default()), "fn_ptr_cast").unwrap();
            let fn_ptr = fn_val.as_global_value().as_pointer_value();
            let fn_as_int = self.builder.build_ptr_to_int(fn_ptr, self.context.i64_type(), "fn_int").unwrap();
            self.builder.build_store(fn_ptr_cast, fn_as_int).unwrap();
        }
        
        // Create class with method data
        let method_data_int2 = self.builder.build_ptr_to_int(method_data_i8ptr, self.context.i64_type(), "method_data_int2").unwrap();
        let class_val = self.builder
            .build_call(self.rt.cool_class_new, &[
                name_ptr.as_pointer_value().into(),
                self.context.i64_type().const_int(method_count as u64, false).into(),
                method_data_int2.into(),
            ], "class")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();

        let obj_val = self.builder
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
        if has_init {
            if let Some(body) = init_body {
                let params = init_params.unwrap_or_default();

                if let Some(&init_fn) = methods.get("__init__") {
                    // Call init with self as first arg
                    let mut args: Vec<BasicMetadataValueEnum<'ctx>> = vec![obj_val.into()];
                    // Skip first param (self) when building args
                    for (i, param) in params.iter().skip(1).enumerate() {
                        // Get from global args buffer
                        let idx = self.context.i32_type().const_int((i + 1) as u64, false);
                        let arg_val = self.builder
                            .build_call(self.rt.cool_get_arg, &[idx.into()], &param.name)
                            .unwrap()
                            .try_as_basic_value()
                            .left()
                            .unwrap()
                            .into_struct_value();
                        args.push(arg_val.into());
                    }
                    self.builder.build_call(init_fn, &args, "").unwrap();
                }
            }
        }

        // Return the object
        let result = self.builder.build_load(self.cv_type, obj_ptr, "result").unwrap().into_struct_value();
        self.builder.build_return(Some(&result)).unwrap();

        // Restore state
        self.locals = saved_locals;
        self.current_fn = saved_fn;
        self.loop_stack = saved_loops;
        if let Some(bb) = saved_bb {
            self.builder.position_at_end(bb);
        }

        // Store class info
        self.classes.insert(name.to_string(), ClassInfo {
            constructor,
            methods,
            attributes,
        });

        // Create a global variable to hold the class reference
        let global_name = format!("__class_{}", name);
        let global = self.module.add_global(self.cv_type, None, &global_name);
        
        // At runtime, we need to initialize this - for now, just store constructor ref
        let constructor_holder = self.builder.build_alloca(self.cv_type, &format!("{}_holder", name)).unwrap();
        
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
        self.builder
            .build_conditional_branch(i1, ok_bb, fail_bb)
            .unwrap();

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

            Expr::Call { callee, args, .. } => self.compile_call(callee, args),

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

            // index access: obj[i]
            Expr::Index { object, index } => {
                let obj_val = self.compile_expr(object)?;
                let idx_val = self.compile_expr(index)?;
                Ok(self.call_binop_fn(self.rt.cool_list_get, obj_val, idx_val, "index"))
            }

            // attribute access: obj.attr
            Expr::Attr { object, name } => {
                let obj_val = self.compile_expr(object)?;
                let attr_name_ptr = self.builder.build_global_string_ptr(name, &format!("attr_{}", name)).unwrap();
                Ok(self.builder
                    .build_call(self.rt.cool_get_attr, &[obj_val.into(), attr_name_ptr.as_pointer_value().into()], "get_attr")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_struct_value())
            }

            other => Err(format!("unsupported expression in LLVM backend: {other:?}")),
        }
    }

    // ── Binary expression ─────────────────────────────────────────────────────

    fn compile_binop_expr(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<StructValue<'ctx>, String> {
        match op {
            BinOp::And => return self.compile_and(left, right),
            BinOp::Or => return self.compile_or(left, right),
            BinOp::In | BinOp::NotIn => {
                return Err("'in'/'not in' not supported in LLVM backend".into());
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
        self.builder
            .build_conditional_branch(i1, rhs_bb, done_bb)
            .unwrap();

        self.builder.position_at_end(rhs_bb);
        let rhs = self.compile_expr(right)?;
        let rhs_end = self.builder.get_insert_block().unwrap();
        self.builder.build_unconditional_branch(done_bb).unwrap();

        self.builder.position_at_end(done_bb);
        let phi = self.builder.build_phi(self.cv_type, "and_res").unwrap();
        phi.add_incoming(&[
            (&lhs as &dyn BasicValue, lhs_end),
            (&rhs as &dyn BasicValue, rhs_end),
        ]);
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
        self.builder
            .build_conditional_branch(i1, done_bb, rhs_bb)
            .unwrap();

        self.builder.position_at_end(rhs_bb);
        let rhs = self.compile_expr(right)?;
        let rhs_end = self.builder.get_insert_block().unwrap();
        self.builder.build_unconditional_branch(done_bb).unwrap();

        self.builder.position_at_end(done_bb);
        let phi = self.builder.build_phi(self.cv_type, "or_res").unwrap();
        phi.add_incoming(&[
            (&lhs as &dyn BasicValue, lhs_end),
            (&rhs as &dyn BasicValue, rhs_end),
        ]);
        Ok(phi.as_basic_value().into_struct_value())
    }

    // ── Function call ─────────────────────────────────────────────────────────

    fn compile_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<StructValue<'ctx>, String> {
        // Handle method calls: obj.method(args)
        if let Expr::Attr { object, name: method_name } = callee {
            let obj_val = self.compile_expr(object)?;
            let attr_name = format!("method_{}", method_name);
            let attr_name_ptr = self.builder.build_global_string_ptr(&attr_name, &attr_name).unwrap();
            
            // Call method - the runtime looks up the method from the class structure
            let i32t = self.context.i32_type();
            let nargs_i32 = i32t.const_int(args.len() as u64, false); // number of args (excluding self, added by runtime)
            
            return Ok(self.builder
                .build_call(self.rt.cool_call_method_vararg, &[
                    obj_val.into(),
                    attr_name_ptr.as_pointer_value().into(),
                    nargs_i32.into(),
                ], "call_method")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_struct_value());
        }

        // Simple function call: name(args)
        let name = match callee {
            Expr::Ident(n) => n.clone(),
            other => {
                return Err(format!(
                    "only named function calls are supported; got {other:?}"
                ))
            }
        };

        // ── Check for class instantiation ───────────────────────────────
        if self.classes.contains_key(&name) {
            // Extract constructor first to avoid borrow conflict
            let constructor = {
                let class_info = self.classes.get(&name).unwrap();
                class_info.constructor
            };
            
            // Compile arguments and store to global buffer for constructor
            let i32t = self.context.i32_type();
            for (i, arg) in args.iter().enumerate() {
                let cv = self.compile_expr(arg)?;
                let idx_val = i32t.const_int(i as u64, false);
                // Store arg to global method args buffer
                self.builder
                    .build_call(self.rt.cool_set_global_arg, &[idx_val.into(), cv.into()], "set_global_arg")
                    .unwrap();
            }
            
            // Call the constructor (which reads args from global buffer)
            return Ok(self.builder
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
                return Err(
                    "asm() requires at least one argument (assembly template string)".into(),
                );
            }
            let template = match &args[0] {
                Expr::Str(s) => s.clone(),
                _ => return Err("asm() first argument must be a string literal".into()),
            };
            let (constraints, operand_start) = if args.len() > 1 {
                match &args[1] {
                    Expr::Str(s) => (s.clone(), 2),
                    _ => {
                        return Err(
                            "asm() second argument must be a string literal (constraints)".into(),
                        )
                    }
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
            self.builder
                .build_call(self.rt.cool_print, &call_args, "")
                .unwrap();
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
            if n < 2 || n > 3 {
                return Err("range() takes 2 or 3 arguments".into());
            }
            let start = self.compile_expr(&args[0])?;
            let stop = self.compile_expr(&args[1])?;
            let step = if n == 3 {
                self.compile_expr(&args[2])?
            } else {
                self.build_int(1)
            };
            return Ok(self
                .builder
                .build_call(
                    self.rt.cool_range,
                    &[start.into(), stop.into(), step.into()],
                    "range",
                )
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

        // ── user-defined function ──
        let fn_val = self
            .functions
            .get(&name)
            .copied()
            .ok_or_else(|| format!("undefined function '{name}'"))?;

        let mut compiled: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
        for arg in args {
            let cv = self.compile_expr(arg)?;
            compiled.push(cv.into());
        }

        Ok(self
            .builder
            .build_call(fn_val, &compiled, "call")
            .unwrap()
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value())
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn compile_program(program: &Program, output_path: &Path) -> Result<(), String> {
    // Initialise LLVM for the host machine
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| format!("LLVM init error: {e}"))?;

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
        }
    }

    // ── Pass 2: build main() and compile all top-level statements ────────────
    let i32_type = context.i32_type();
    let main_fn = compiler
        .module
        .add_function("main", i32_type.fn_type(&[], false), None);
    let entry = context.append_basic_block(main_fn, "entry");
    compiler.builder.position_at_end(entry);
    compiler.current_fn = Some(main_fn);

    compiler.compile_stmts(program)?;

    // Ensure main is properly terminated
    if !compiler.current_block_terminated() {
        let zero = i32_type.const_int(0, false);
        compiler.builder.build_return(Some(&zero)).unwrap();
    }

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

    std::fs::write(rt_c_path, RUNTIME_C)
        .map_err(|e| format!("Failed to write runtime source: {e}"))?;

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
