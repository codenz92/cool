/* cool_runtime.c — embedded C runtime for the Cool LLVM backend
   Compiled to a .o by cc, then linked with the LLVM-emitted .o.        */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <ctype.h>

/* ── Tag constants ──────────────────────────────────────────────────────── */
#define TAG_NIL   0
#define TAG_INT   1
#define TAG_FLOAT 2
#define TAG_BOOL  3
#define TAG_STR   4
#define TAG_LIST  5
#define TAG_DICT  6
#define TAG_INST  7
#define TAG_FILE  8

/* ── CoolVal  (forward-declared so CoolList/CoolDict can reference it) ── */
typedef struct CoolVal CoolVal;
struct CoolVal {
    int32_t tag;
    int64_t payload;
};

/* ── Internal float bit-pattern helpers ─────────────────────────────────── */
static double cv_as_float(CoolVal v) {
    double d; memcpy(&d, &v.payload, sizeof(double)); return d;
}
static double cv_to_float(CoolVal v) {
    if (v.tag == TAG_FLOAT) return cv_as_float(v);
    if (v.tag == TAG_INT)   return (double)v.payload;
    return 0.0;
}

/* ── Constructors ───────────────────────────────────────────────────────── */
CoolVal cv_nil(void)           { CoolVal v; v.tag = TAG_NIL;   v.payload = 0;                    return v; }
CoolVal cv_int(int64_t n)      { CoolVal v; v.tag = TAG_INT;   v.payload = n;                    return v; }
CoolVal cv_bool(int32_t b)     { CoolVal v; v.tag = TAG_BOOL;  v.payload = b ? 1 : 0;            return v; }
CoolVal cv_str(const char* s)  { CoolVal v; v.tag = TAG_STR;   v.payload = (int64_t)(intptr_t)s; return v; }
CoolVal cv_float(double f) {
    CoolVal v; v.tag = TAG_FLOAT;
    memcpy(&v.payload, &f, sizeof(double)); return v;
}

/* ── Internal equality (used by contains / dict lookup) ─────────────────── */
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

/* ═══════════════════════════════════════════════════════════════════════
   COOLLIST  — dynamic array of CoolVal
   ═══════════════════════════════════════════════════════════════════════ */
typedef struct {
    int64_t  len, cap;
    CoolVal* data;
} CoolList;

static CoolList* cv_list_ptr(CoolVal v) { return (CoolList*)(intptr_t)v.payload; }

static void list_ensure(CoolList* l) {
    if (l->len >= l->cap) {
        l->cap *= 2;
        l->data = realloc(l->data, (size_t)l->cap * sizeof(CoolVal));
    }
}

/* exported helpers used by codegen ─────────────────────────────────────── */
void cool_list_append_raw(CoolVal list, CoolVal item) {
    CoolList* l = cv_list_ptr(list);
    list_ensure(l);
    l->data[l->len++] = item;
}

CoolVal cool_list_new(void) {
    CoolList* l = malloc(sizeof(CoolList));
    l->len = 0; l->cap = 8;
    l->data = malloc(8 * sizeof(CoolVal));
    CoolVal v; v.tag = TAG_LIST; v.payload = (int64_t)(intptr_t)l; return v;
}

/* cool_list_of(n, v0, v1, ...) — variadic literal constructor */
CoolVal cool_list_of(int32_t n, ...) {
    CoolVal v = cool_list_new();
    va_list ap; va_start(ap, n);
    for (int32_t i = 0; i < n; i++) cool_list_append_raw(v, va_arg(ap, CoolVal));
    va_end(ap); return v;
}

static int64_t norm_idx(int64_t i, int64_t len) {
    if (i < 0) i += len; return i;
}

CoolVal cool_list_get(CoolVal list, CoolVal idx_val) {
    CoolList* l = cv_list_ptr(list);
    int64_t i = norm_idx(idx_val.payload, l->len);
    if (i < 0 || i >= l->len) { fputs("IndexError: list index out of range\n", stderr); exit(1); }
    return l->data[i];
}

CoolVal cool_list_set(CoolVal list, CoolVal idx_val, CoolVal val) {
    CoolList* l = cv_list_ptr(list);
    int64_t i = norm_idx(idx_val.payload, l->len);
    if (i < 0 || i >= l->len) { fputs("IndexError: list assignment out of range\n", stderr); exit(1); }
    l->data[i] = val; return cv_nil();
}

CoolVal cool_list_len(CoolVal list)  { return cv_int(cv_list_ptr(list)->len); }

CoolVal cool_list_append(CoolVal list, CoolVal item) {
    cool_list_append_raw(list, item); return cv_nil();
}

CoolVal cool_list_pop(CoolVal list, CoolVal idx_val) {
    CoolList* l = cv_list_ptr(list);
    int64_t i = (idx_val.tag == TAG_INT) ? norm_idx(idx_val.payload, l->len) : l->len - 1;
    if (i < 0 || i >= l->len) { fputs("IndexError: pop index out of range\n", stderr); exit(1); }
    CoolVal ret = l->data[i];
    memmove(&l->data[i], &l->data[i+1], (size_t)(l->len - i - 1) * sizeof(CoolVal));
    l->len--; return ret;
}

CoolVal cool_list_slice(CoolVal list, CoolVal start_v, CoolVal stop_v) {
    CoolList* l = cv_list_ptr(list);
    int64_t len = l->len;
    int64_t s = (start_v.tag == TAG_NIL) ? 0   : norm_idx(start_v.payload, len);
    int64_t e = (stop_v.tag  == TAG_NIL) ? len : norm_idx(stop_v.payload,  len);
    if (s < 0) s = 0; if (e > len) e = len;
    CoolVal res = cool_list_new();
    for (int64_t i = s; i < e; i++) cool_list_append_raw(res, l->data[i]);
    return res;
}

CoolVal cool_list_contains(CoolVal list, CoolVal item) {
    CoolList* l = cv_list_ptr(list);
    for (int64_t i = 0; i < l->len; i++)
        if (cv_eq_raw(l->data[i], item)) return cv_bool(1);
    return cv_bool(0);
}

static int cmp_coolval(const void* a, const void* b) {
    CoolVal va = *(const CoolVal*)a, vb = *(const CoolVal*)b;
    if (va.tag == TAG_STR && vb.tag == TAG_STR)
        return strcmp((const char*)(intptr_t)va.payload, (const char*)(intptr_t)vb.payload);
    double fa = cv_to_float(va), fb = cv_to_float(vb);
    return (fa > fb) - (fa < fb);
}
static int cmp_coolval_rev(const void* a, const void* b) { return cmp_coolval(b, a); }

CoolVal cool_list_sort(CoolVal list, CoolVal rev) {
    CoolList* l = cv_list_ptr(list);
    qsort(l->data, (size_t)l->len, sizeof(CoolVal),
          (rev.tag == TAG_BOOL && rev.payload) ? cmp_coolval_rev : cmp_coolval);
    return cv_nil();
}
CoolVal cool_list_reverse(CoolVal list) {
    CoolList* l = cv_list_ptr(list);
    for (int64_t i = 0, j = l->len - 1; i < j; i++, j--) {
        CoolVal t = l->data[i]; l->data[i] = l->data[j]; l->data[j] = t;
    }
    return cv_nil();
}
CoolVal cool_list_index(CoolVal list, CoolVal item) {
    CoolList* l = cv_list_ptr(list);
    for (int64_t i = 0; i < l->len; i++)
        if (cv_eq_raw(l->data[i], item)) return cv_int(i);
    fputs("ValueError: item not in list\n", stderr); exit(1);
}
CoolVal cool_list_count(CoolVal list, CoolVal item) {
    CoolList* l = cv_list_ptr(list); int64_t c = 0;
    for (int64_t i = 0; i < l->len; i++) if (cv_eq_raw(l->data[i], item)) c++;
    return cv_int(c);
}
CoolVal cool_list_extend(CoolVal list, CoolVal other) {
    CoolList* lo = cv_list_ptr(other);
    for (int64_t i = 0; i < lo->len; i++) cool_list_append_raw(list, lo->data[i]);
    return cv_nil();
}
CoolVal cool_list_insert(CoolVal list, CoolVal idx_v, CoolVal item) {
    CoolList* l = cv_list_ptr(list);
    int64_t i = norm_idx(idx_v.payload, l->len);
    if (i < 0) i = 0; if (i > l->len) i = l->len;
    list_ensure(l);
    memmove(&l->data[i+1], &l->data[i], (size_t)(l->len - i) * sizeof(CoolVal));
    l->data[i] = item; l->len++; return cv_nil();
}
CoolVal cool_list_remove(CoolVal list, CoolVal item) {
    CoolList* l = cv_list_ptr(list);
    for (int64_t i = 0; i < l->len; i++) {
        if (cv_eq_raw(l->data[i], item)) {
            memmove(&l->data[i], &l->data[i+1], (size_t)(l->len-i-1)*sizeof(CoolVal));
            l->len--; return cv_nil();
        }
    }
    fputs("ValueError: list.remove: item not found\n", stderr); exit(1);
}
CoolVal cool_list_clear(CoolVal list) { cv_list_ptr(list)->len = 0; return cv_nil(); }
CoolVal cool_list_copy(CoolVal list) {
    CoolList* l = cv_list_ptr(list); CoolVal res = cool_list_new();
    for (int64_t i = 0; i < l->len; i++) cool_list_append_raw(res, l->data[i]);
    return res;
}

/* ═══════════════════════════════════════════════════════════════════════
   COOLDICT  — insertion-ordered linear map (string keys usual case)
   ═══════════════════════════════════════════════════════════════════════ */
typedef struct {
    int64_t  len, cap;
    CoolVal* keys;
    CoolVal* vals;
} CoolDict;

static CoolDict* cv_dict_ptr(CoolVal v) { return (CoolDict*)(intptr_t)v.payload; }

static void dict_ensure(CoolDict* d) {
    if (d->len >= d->cap) {
        d->cap *= 2;
        d->keys = realloc(d->keys, (size_t)d->cap * sizeof(CoolVal));
        d->vals = realloc(d->vals, (size_t)d->cap * sizeof(CoolVal));
    }
}

void cool_dict_set_raw(CoolDict* d, CoolVal key, CoolVal val) {
    for (int64_t i = 0; i < d->len; i++) {
        if (cv_eq_raw(d->keys[i], key)) { d->vals[i] = val; return; }
    }
    dict_ensure(d);
    d->keys[d->len] = key; d->vals[d->len] = val; d->len++;
}

CoolVal cool_dict_new(void) {
    CoolDict* d = malloc(sizeof(CoolDict));
    d->len = 0; d->cap = 8;
    d->keys = malloc(8 * sizeof(CoolVal));
    d->vals = malloc(8 * sizeof(CoolVal));
    CoolVal v; v.tag = TAG_DICT; v.payload = (int64_t)(intptr_t)d; return v;
}

CoolVal cool_dict_of(int32_t n_pairs, ...) {
    CoolVal v = cool_dict_new();
    va_list ap; va_start(ap, n_pairs);
    for (int32_t i = 0; i < n_pairs; i++) {
        CoolVal k = va_arg(ap, CoolVal), val = va_arg(ap, CoolVal);
        cool_dict_set_raw(cv_dict_ptr(v), k, val);
    }
    va_end(ap); return v;
}

CoolVal cool_dict_get(CoolVal dict, CoolVal key) {
    CoolDict* d = cv_dict_ptr(dict);
    for (int64_t i = 0; i < d->len; i++)
        if (cv_eq_raw(d->keys[i], key)) return d->vals[i];
    return cv_nil();
}
CoolVal cool_dict_get_default(CoolVal dict, CoolVal key, CoolVal def) {
    CoolDict* d = cv_dict_ptr(dict);
    for (int64_t i = 0; i < d->len; i++)
        if (cv_eq_raw(d->keys[i], key)) return d->vals[i];
    return def;
}
CoolVal cool_dict_set(CoolVal dict, CoolVal key, CoolVal val) {
    cool_dict_set_raw(cv_dict_ptr(dict), key, val); return cv_nil();
}
CoolVal cool_dict_contains(CoolVal dict, CoolVal key) {
    CoolDict* d = cv_dict_ptr(dict);
    for (int64_t i = 0; i < d->len; i++)
        if (cv_eq_raw(d->keys[i], key)) return cv_bool(1);
    return cv_bool(0);
}
CoolVal cool_dict_len(CoolVal dict) { return cv_int(cv_dict_ptr(dict)->len); }
CoolVal cool_dict_keys(CoolVal dict) {
    CoolDict* d = cv_dict_ptr(dict); CoolVal res = cool_list_new();
    for (int64_t i = 0; i < d->len; i++) cool_list_append_raw(res, d->keys[i]);
    return res;
}
CoolVal cool_dict_values(CoolVal dict) {
    CoolDict* d = cv_dict_ptr(dict); CoolVal res = cool_list_new();
    for (int64_t i = 0; i < d->len; i++) cool_list_append_raw(res, d->vals[i]);
    return res;
}
CoolVal cool_dict_items(CoolVal dict) {
    CoolDict* d = cv_dict_ptr(dict); CoolVal res = cool_list_new();
    for (int64_t i = 0; i < d->len; i++) {
        CoolVal pair = cool_list_of(2, d->keys[i], d->vals[i]);
        cool_list_append_raw(res, pair);
    }
    return res;
}
CoolVal cool_dict_remove(CoolVal dict, CoolVal key) {
    CoolDict* d = cv_dict_ptr(dict);
    for (int64_t i = 0; i < d->len; i++) {
        if (cv_eq_raw(d->keys[i], key)) {
            memmove(&d->keys[i], &d->keys[i+1], (size_t)(d->len-i-1)*sizeof(CoolVal));
            memmove(&d->vals[i], &d->vals[i+1], (size_t)(d->len-i-1)*sizeof(CoolVal));
            d->len--; return cv_nil();
        }
    }
    fputs("KeyError\n", stderr); exit(1);
}
CoolVal cool_dict_clear(CoolVal dict)  { cv_dict_ptr(dict)->len = 0; return cv_nil(); }
CoolVal cool_dict_update(CoolVal dict, CoolVal other) {
    CoolDict* d = cv_dict_ptr(other);
    for (int64_t i = 0; i < d->len; i++)
        cool_dict_set_raw(cv_dict_ptr(dict), d->keys[i], d->vals[i]);
    return cv_nil();
}
CoolVal cool_dict_copy(CoolVal dict) {
    CoolDict* d = cv_dict_ptr(dict);
    CoolVal res = cool_dict_new();
    CoolDict* out = cv_dict_ptr(res);
    for (int64_t i = 0; i < d->len; i++) cool_dict_set_raw(out, d->keys[i], d->vals[i]);
    return res;
}
CoolVal cool_dict_pop(CoolVal dict, CoolVal key) {
    CoolDict* d = cv_dict_ptr(dict);
    for (int64_t i = 0; i < d->len; i++) {
        if (cv_eq_raw(d->keys[i], key)) {
            CoolVal ret = d->vals[i];
            memmove(&d->keys[i], &d->keys[i+1], (size_t)(d->len-i-1)*sizeof(CoolVal));
            memmove(&d->vals[i], &d->vals[i+1], (size_t)(d->len-i-1)*sizeof(CoolVal));
            d->len--; return ret;
        }
    }
    fputs("KeyError\n", stderr); exit(1);
}

/* ═══════════════════════════════════════════════════════════════════════
   COOLINST  — class instance  (fields stored as an embedded CoolDict)
   ═══════════════════════════════════════════════════════════════════════ */
typedef struct {
    int32_t  class_id;
    CoolDict fields;
} CoolInst;

static CoolInst* cv_inst_ptr(CoolVal v) { return (CoolInst*)(intptr_t)v.payload; }

CoolVal cool_inst_new(int32_t class_id) {
    CoolInst* inst = calloc(1, sizeof(CoolInst));
    inst->class_id = class_id;
    inst->fields.len = 0; inst->fields.cap = 8;
    inst->fields.keys = malloc(8 * sizeof(CoolVal));
    inst->fields.vals = malloc(8 * sizeof(CoolVal));
    CoolVal v; v.tag = TAG_INST; v.payload = (int64_t)(intptr_t)inst; return v;
}
CoolVal cool_inst_get_field(CoolVal inst, CoolVal key) {
    CoolInst* i = cv_inst_ptr(inst);
    for (int64_t j = 0; j < i->fields.len; j++)
        if (cv_eq_raw(i->fields.keys[j], key)) return i->fields.vals[j];
    return cv_nil();
}
CoolVal cool_inst_set_field(CoolVal inst, CoolVal key, CoolVal val) {
    cool_dict_set_raw(&cv_inst_ptr(inst)->fields, key, val); return cv_nil();
}
int32_t cool_inst_class_id(CoolVal inst) { return cv_inst_ptr(inst)->class_id; }

/* ═══════════════════════════════════════════════════════════════════════
   METHOD DISPATCH TABLE
   Methods registered from main() before user code runs.
   Signature of every registered method:
     CoolVal fn(CoolVal self, int32_t argc, CoolVal* argv)
   ═══════════════════════════════════════════════════════════════════════ */
typedef CoolVal (*MethodFn)(CoolVal, int32_t, CoolVal*);

typedef struct { int32_t class_id; const char* name; MethodFn fn; } MethodEntry;

#define MAX_METHODS 4096
#define MAX_CLASSES 512
static MethodEntry g_methods[MAX_METHODS];
static int32_t     g_n_methods = 0;
static int32_t     g_parent_of[MAX_CLASSES];

/* Called from main() preamble generated by Rust codegen */
void cool_register_method(int32_t class_id, const char* name, void* fn) {
    if (g_n_methods < MAX_METHODS) {
        g_methods[g_n_methods].class_id = class_id;
        g_methods[g_n_methods].name     = name;
        g_methods[g_n_methods].fn       = (MethodFn)fn;
        g_n_methods++;
    }
}
void cool_set_parent(int32_t class_id, int32_t parent_id) {
    if (class_id >= 0 && class_id < MAX_CLASSES) g_parent_of[class_id] = parent_id;
}

/* Forward-declare dispatch (defined further down) */
static CoolVal dispatch_on_inst(CoolVal self, int32_t cls, const char* name, int32_t argc, CoolVal* argv);

/* ═══════════════════════════════════════════════════════════════════════
   TRUTHINESS
   ═══════════════════════════════════════════════════════════════════════ */
int32_t cool_truthy(CoolVal v) {
    switch (v.tag) {
        case TAG_NIL:   return 0;
        case TAG_INT:   return v.payload != 0 ? 1 : 0;
        case TAG_FLOAT: return cv_as_float(v) != 0.0 ? 1 : 0;
        case TAG_BOOL:  return v.payload != 0 ? 1 : 0;
        case TAG_STR:   return ((const char*)(intptr_t)v.payload)[0] != '\0' ? 1 : 0;
        case TAG_LIST:  return cv_list_ptr(v)->len != 0 ? 1 : 0;
        case TAG_DICT:  return cv_dict_ptr(v)->len != 0 ? 1 : 0;
        case TAG_INST:  return 1;
        case TAG_FILE:  return 1;
        default:        return 0;
    }
}

/* ═══════════════════════════════════════════════════════════════════════
   ARITHMETIC
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_add(CoolVal a, CoolVal b) {
    if (a.tag == TAG_STR && b.tag == TAG_STR) {
        const char* sa = (const char*)(intptr_t)a.payload;
        const char* sb = (const char*)(intptr_t)b.payload;
        size_t la = strlen(sa), lb = strlen(sb);
        char* r = malloc(la + lb + 1);
        memcpy(r, sa, la); memcpy(r + la, sb, lb); r[la + lb] = '\0';
        return cv_str(r);
    }
    if (a.tag == TAG_LIST && b.tag == TAG_LIST) return cool_list_extend(cool_list_copy(a), b);
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
    /* str * int */
    if (a.tag == TAG_STR && b.tag == TAG_INT) {
        const char* s = (const char*)(intptr_t)a.payload;
        size_t n = strlen(s); int64_t times = b.payload;
        if (times <= 0) return cv_str("");
        char* r = malloc(n * (size_t)times + 1);
        for (int64_t i = 0; i < times; i++) memcpy(r + i * n, s, n);
        r[n * (size_t)times] = '\0'; return cv_str(r);
    }
    return cv_int(a.payload * b.payload);
}
CoolVal cool_div(CoolVal a, CoolVal b) { return cv_float(cv_to_float(a) / cv_to_float(b)); }
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
CoolVal cool_pow(CoolVal a, CoolVal b) { return cv_float(pow(cv_to_float(a), cv_to_float(b))); }
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

/* ═══════════════════════════════════════════════════════════════════════
   COMPARISONS
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_eq(CoolVal a, CoolVal b)   { return cv_bool(cv_eq_raw(a, b)); }
CoolVal cool_neq(CoolVal a, CoolVal b)  { return cv_bool(!cv_eq_raw(a, b)); }

#define STR_CMP(op) \
    if (a.tag == TAG_STR && b.tag == TAG_STR) \
        return cv_bool(strcmp((const char*)(intptr_t)a.payload, \
                              (const char*)(intptr_t)b.payload) op 0); \
    return cv_bool(cv_to_float(a) op cv_to_float(b))

CoolVal cool_lt(CoolVal a, CoolVal b)   { STR_CMP(<);  }
CoolVal cool_lteq(CoolVal a, CoolVal b) { STR_CMP(<=); }
CoolVal cool_gt(CoolVal a, CoolVal b)   { STR_CMP(>);  }
CoolVal cool_gteq(CoolVal a, CoolVal b) { STR_CMP(>=); }

/* ═══════════════════════════════════════════════════════════════════════
   LOGIC / BITWISE
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_not(CoolVal a)               { return cv_bool(!cool_truthy(a)); }
CoolVal cool_bitand(CoolVal a, CoolVal b) { return cv_int(a.payload & b.payload); }
CoolVal cool_bitor(CoolVal a, CoolVal b)  { return cv_int(a.payload | b.payload); }
CoolVal cool_bitxor(CoolVal a, CoolVal b) { return cv_int(a.payload ^ b.payload); }
CoolVal cool_bitnot(CoolVal a)            { return cv_int(~a.payload); }
CoolVal cool_lshift(CoolVal a, CoolVal b) { return cv_int(a.payload << (int)b.payload); }
CoolVal cool_rshift(CoolVal a, CoolVal b) { return cv_int(a.payload >> (int)b.payload); }

/* ═══════════════════════════════════════════════════════════════════════
   TYPE NAME / TO_STR
   ═══════════════════════════════════════════════════════════════════════ */
static const char* tag_name(int32_t tag) {
    switch (tag) {
        case TAG_NIL:  return "nil";
        case TAG_INT:  return "int";
        case TAG_FLOAT:return "float";
        case TAG_BOOL: return "bool";
        case TAG_STR:  return "str";
        case TAG_LIST: return "list";
        case TAG_DICT: return "dict";
        case TAG_INST: return "instance";
        case TAG_FILE: return "file";
        default:       return "unknown";
    }
}
CoolVal cool_type_name(CoolVal v) { return cv_str(tag_name(v.tag)); }

/* cool_to_str — returns malloc'd or static char* (callers must treat as read-only) */
char* cool_to_str(CoolVal v) {
    if (v.tag == TAG_STR) return (char*)(intptr_t)v.payload;
    if (v.tag == TAG_NIL) return "nil";
    if (v.tag == TAG_BOOL) return v.payload ? "true" : "false";
    char* buf = malloc(64);
    if (!buf) return (char*)"<oom>";
    if (v.tag == TAG_INT)   { snprintf(buf, 64, "%lld", (long long)v.payload); return buf; }
    if (v.tag == TAG_FLOAT) { snprintf(buf, 64, "%g",   cv_as_float(v));       return buf; }
    if (v.tag == TAG_LIST) {
        CoolList* l = cv_list_ptr(v);
        /* Build "[a, b, c]" */
        char* out = malloc(64); size_t cap = 64, len = 0;
        out[len++] = '[';
        for (int64_t i = 0; i < l->len; i++) {
            if (i > 0) { if (len + 2 >= cap) { cap *= 2; out = realloc(out, cap); }
                         out[len++] = ','; out[len++] = ' '; }
            char* elem = cool_to_str(l->data[i]);
            size_t el = strlen(elem);
            while (len + el + 2 >= cap) { cap *= 2; out = realloc(out, cap); }
            memcpy(out + len, elem, el); len += el;
        }
        if (len + 2 >= cap) { cap += 4; out = realloc(out, cap); }
        out[len++] = ']'; out[len] = '\0';
        free(buf); return out;
    }
    if (v.tag == TAG_DICT) {
        CoolDict* d = cv_dict_ptr(v);
        char* out = malloc(64); size_t cap = 64, len = 0;
        out[len++] = '{';
        for (int64_t i = 0; i < d->len; i++) {
            if (i > 0) { if (len + 2 >= cap) { cap *= 2; out = realloc(out, cap); }
                         out[len++] = ','; out[len++] = ' '; }
            char* k = cool_to_str(d->keys[i]);
            char* val = cool_to_str(d->vals[i]);
            size_t kl = strlen(k), vl = strlen(val);
            while (len + kl + vl + 4 >= cap) { cap *= 2; out = realloc(out, cap); }
            memcpy(out+len, k, kl); len += kl;
            out[len++] = ':'; out[len++] = ' ';
            memcpy(out+len, val, vl); len += vl;
        }
        if (len + 2 >= cap) { cap += 4; out = realloc(out, cap); }
        out[len++] = '}'; out[len] = '\0';
        free(buf); return out;
    }
    if (v.tag == TAG_INST) {
        snprintf(buf, 64, "<instance class_id=%d>", cv_inst_ptr(v)->class_id);
        return buf;
    }
    snprintf(buf, 64, "<%s>", tag_name(v.tag));
    return buf;
}

/* ═══════════════════════════════════════════════════════════════════════
   COOL_LEN — len() builtin
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_len(CoolVal v) {
    switch (v.tag) {
        case TAG_STR:  return cv_int((int64_t)strlen((const char*)(intptr_t)v.payload));
        case TAG_LIST: return cool_list_len(v);
        case TAG_DICT: return cool_dict_len(v);
        default:
            fprintf(stderr, "TypeError: object of type '%s' has no len()\n", tag_name(v.tag));
            exit(1);
    }
}

/* ═══════════════════════════════════════════════════════════════════════
   COOL_CONTAINS — 'in' operator
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_contains(CoolVal container, CoolVal item) {
    if (container.tag == TAG_LIST) return cool_list_contains(container, item);
    if (container.tag == TAG_DICT) return cool_dict_contains(container, item);
    if (container.tag == TAG_STR && item.tag == TAG_STR) {
        const char* haystack = (const char*)(intptr_t)container.payload;
        const char* needle   = (const char*)(intptr_t)item.payload;
        return cv_bool(strstr(haystack, needle) != NULL);
    }
    fprintf(stderr, "TypeError: 'in' not supported for type '%s'\n", tag_name(container.tag));
    exit(1);
}

/* ═══════════════════════════════════════════════════════════════════════
   COOL_INDEX — subscript operator  obj[key]
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_index(CoolVal obj, CoolVal key) {
    if (obj.tag == TAG_LIST) return cool_list_get(obj, key);
    if (obj.tag == TAG_DICT) {
        CoolVal r = cool_dict_get(obj, key);
        if (r.tag == TAG_NIL && !cv_eq_raw(cool_dict_get(obj, key), cv_nil())) {
            /* key legitimately maps to nil — return it */
        }
        return r;
    }
    if (obj.tag == TAG_STR) {
        const char* s = (const char*)(intptr_t)obj.payload;
        int64_t len = (int64_t)strlen(s);
        int64_t i = norm_idx(key.payload, len);
        if (i < 0 || i >= len) { fputs("IndexError: string index out of range\n", stderr); exit(1); }
        char* r = malloc(2); r[0] = s[i]; r[1] = '\0'; return cv_str(r);
    }
    fprintf(stderr, "TypeError: '%s' object is not subscriptable\n", tag_name(obj.tag));
    exit(1);
}

/* ═══════════════════════════════════════════════════════════════════════
   COOL_SLICE  — obj[start:stop]
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_slice(CoolVal obj, CoolVal start_v, CoolVal stop_v) {
    if (obj.tag == TAG_LIST) return cool_list_slice(obj, start_v, stop_v);
    if (obj.tag == TAG_STR) {
        const char* s = (const char*)(intptr_t)obj.payload;
        int64_t len = (int64_t)strlen(s);
        int64_t ss = (start_v.tag == TAG_NIL) ? 0   : norm_idx(start_v.payload, len);
        int64_t se = (stop_v.tag  == TAG_NIL) ? len : norm_idx(stop_v.payload,  len);
        if (ss < 0) ss = 0; if (se > len) se = len; if (se < ss) se = ss;
        size_t size = (size_t)(se - ss);
        char* r = malloc(size + 1);
        memcpy(r, s + ss, size); r[size] = '\0'; return cv_str(r);
    }
    fprintf(stderr, "TypeError: '%s' object is not sliceable\n", tag_name(obj.tag));
    exit(1);
}

/* ═══════════════════════════════════════════════════════════════════════
   STRING METHODS
   ═══════════════════════════════════════════════════════════════════════ */
static const char* as_cstr(CoolVal v) { return (const char*)(intptr_t)v.payload; }

CoolVal cool_str_upper(CoolVal s) {
    const char* src = as_cstr(s); size_t n = strlen(src);
    char* r = malloc(n + 1);
    for (size_t i = 0; i <= n; i++) r[i] = (char)toupper((unsigned char)src[i]);
    return cv_str(r);
}
CoolVal cool_str_lower(CoolVal s) {
    const char* src = as_cstr(s); size_t n = strlen(src);
    char* r = malloc(n + 1);
    for (size_t i = 0; i <= n; i++) r[i] = (char)tolower((unsigned char)src[i]);
    return cv_str(r);
}
static CoolVal _strip(const char* src, int left, int right) {
    size_t n = strlen(src);
    size_t s = 0, e = n;
    if (left)  while (s < e && isspace((unsigned char)src[s])) s++;
    if (right) while (e > s && isspace((unsigned char)src[e-1])) e--;
    size_t len = e - s;
    char* r = malloc(len + 1); memcpy(r, src + s, len); r[len] = '\0';
    return cv_str(r);
}
CoolVal cool_str_strip(CoolVal s)  { return _strip(as_cstr(s), 1, 1); }
CoolVal cool_str_lstrip(CoolVal s) { return _strip(as_cstr(s), 1, 0); }
CoolVal cool_str_rstrip(CoolVal s) { return _strip(as_cstr(s), 0, 1); }

CoolVal cool_str_find(CoolVal s, CoolVal sub) {
    const char* haystack = as_cstr(s);
    const char* needle   = as_cstr(sub);
    const char* p = strstr(haystack, needle);
    return p ? cv_int((int64_t)(p - haystack)) : cv_int(-1);
}
CoolVal cool_str_startswith(CoolVal s, CoolVal prefix) {
    const char* str = as_cstr(s), *pre = as_cstr(prefix);
    return cv_bool(strncmp(str, pre, strlen(pre)) == 0);
}
CoolVal cool_str_endswith(CoolVal s, CoolVal suffix) {
    const char* str = as_cstr(s), *suf = as_cstr(suffix);
    size_t sl = strlen(str), sul = strlen(suf);
    if (sul > sl) return cv_bool(0);
    return cv_bool(strcmp(str + sl - sul, suf) == 0);
}
CoolVal cool_str_replace(CoolVal s, CoolVal old_v, CoolVal new_v) {
    const char* src = as_cstr(s), *old = as_cstr(old_v), *nw = as_cstr(new_v);
    size_t olen = strlen(old), nlen = strlen(new_v.tag == TAG_STR ? as_cstr(new_v) : ""), srclen = strlen(src);
    nlen = strlen(nw);
    if (olen == 0) return s; /* no replacement */
    /* Count occurrences */
    size_t count = 0;
    const char* p = src;
    while ((p = strstr(p, old)) != NULL) { count++; p += olen; }
    size_t cap = srclen + count * (nlen > olen ? nlen - olen : 0) + 1;
    char* r = malloc(cap); size_t ri = 0;
    p = src;
    const char* q;
    while ((q = strstr(p, old)) != NULL) {
        size_t before = (size_t)(q - p);
        memcpy(r + ri, p, before); ri += before;
        memcpy(r + ri, nw, nlen);  ri += nlen;
        p = q + olen;
    }
    size_t rest = strlen(p);
    memcpy(r + ri, p, rest); r[ri + rest] = '\0';
    return cv_str(r);
}
CoolVal cool_str_split(CoolVal s, CoolVal sep_v) {
    CoolVal res = cool_list_new();
    const char* src = as_cstr(s);
    if (sep_v.tag == TAG_NIL) {
        /* split on whitespace */
        const char* p = src;
        while (*p) {
            while (*p && isspace((unsigned char)*p)) p++;
            if (!*p) break;
            const char* start = p;
            while (*p && !isspace((unsigned char)*p)) p++;
            size_t len = (size_t)(p - start);
            char* tok = malloc(len + 1); memcpy(tok, start, len); tok[len] = '\0';
            cool_list_append_raw(res, cv_str(tok));
        }
    } else {
        const char* sep = as_cstr(sep_v);
        size_t sl = strlen(sep);
        const char* p = src;
        while (1) {
            const char* q = strstr(p, sep);
            size_t len = q ? (size_t)(q - p) : strlen(p);
            char* tok = malloc(len + 1); memcpy(tok, p, len); tok[len] = '\0';
            cool_list_append_raw(res, cv_str(tok));
            if (!q) break;
            p = q + sl;
        }
    }
    return res;
}
CoolVal cool_str_join(CoolVal sep, CoolVal lst) {
    const char* s = as_cstr(sep);
    size_t sl = strlen(s);
    CoolList* l = cv_list_ptr(lst);
    size_t total = 0;
    for (int64_t i = 0; i < l->len; i++) {
        if (i > 0) total += sl;
        total += strlen(cool_to_str(l->data[i]));
    }
    char* r = malloc(total + 1); size_t ri = 0;
    for (int64_t i = 0; i < l->len; i++) {
        if (i > 0) { memcpy(r + ri, s, sl); ri += sl; }
        const char* elem = cool_to_str(l->data[i]);
        size_t el = strlen(elem);
        memcpy(r + ri, elem, el); ri += el;
    }
    r[ri] = '\0'; return cv_str(r);
}
CoolVal cool_str_count(CoolVal s, CoolVal sub) {
    const char* haystack = as_cstr(s), *needle = as_cstr(sub);
    size_t nl = strlen(needle); int64_t count = 0;
    if (nl == 0) return cv_int(0);
    const char* p = haystack;
    while ((p = strstr(p, needle)) != NULL) { count++; p += nl; }
    return cv_int(count);
}
CoolVal cool_str_format(CoolVal fmt, CoolVal args) {
    /* Simple positional {}, {0}, {1}, ... */
    const char* f = as_cstr(fmt);
    CoolList* largs = (args.tag == TAG_LIST) ? cv_list_ptr(args) : NULL;
    char* out = malloc(1024); size_t cap = 1024, ri = 0;
    int idx = 0;
    for (const char* p = f; *p; p++) {
        if (*p == '{' && *(p+1) == '}') {
            char* v = (largs && idx < largs->len) ? cool_to_str(largs->data[idx++]) : "";
            size_t vl = strlen(v);
            while (ri + vl + 2 >= cap) { cap *= 2; out = realloc(out, cap); }
            memcpy(out + ri, v, vl); ri += vl; p++;
        } else {
            if (ri + 2 >= cap) { cap *= 2; out = realloc(out, cap); }
            out[ri++] = *p;
        }
    }
    out[ri] = '\0'; return cv_str(out);
}
CoolVal cool_str_title(CoolVal s) {
    const char* src = as_cstr(s); size_t n = strlen(src);
    char* r = malloc(n + 1);
    int prev_space = 1;
    for (size_t i = 0; i <= n; i++) {
        if (i == n) { r[i] = '\0'; break; }
        r[i] = prev_space ? (char)toupper((unsigned char)src[i]) : (char)tolower((unsigned char)src[i]);
        prev_space = isspace((unsigned char)src[i]);
    }
    return cv_str(r);
}
CoolVal cool_str_capitalize(CoolVal s) {
    const char* src = as_cstr(s); size_t n = strlen(src);
    char* r = malloc(n + 1);
    for (size_t i = 0; i < n; i++)
        r[i] = (i == 0) ? (char)toupper((unsigned char)src[i]) : (char)tolower((unsigned char)src[i]);
    r[n] = '\0'; return cv_str(r);
}
CoolVal cool_str_isdigit(CoolVal s) {
    const char* src = as_cstr(s);
    if (*src == '\0') return cv_bool(0);
    for (const char* p = src; *p; p++) if (!isdigit((unsigned char)*p)) return cv_bool(0);
    return cv_bool(1);
}
CoolVal cool_str_isalpha(CoolVal s) {
    const char* src = as_cstr(s);
    if (*src == '\0') return cv_bool(0);
    for (const char* p = src; *p; p++) if (!isalpha((unsigned char)*p)) return cv_bool(0);
    return cv_bool(1);
}
CoolVal cool_str_isspace(CoolVal s) {
    const char* src = as_cstr(s);
    if (*src == '\0') return cv_bool(0);
    for (const char* p = src; *p; p++) if (!isspace((unsigned char)*p)) return cv_bool(0);
    return cv_bool(1);
}
CoolVal cool_str_len(CoolVal s) { return cv_int((int64_t)strlen(as_cstr(s))); }

/* ═══════════════════════════════════════════════════════════════════════
   F-STRING — concatenate n char* parts
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_fstring(int32_t n, ...) {
    va_list ap; va_start(ap, n);
    /* First pass: compute total size */
    size_t total = 0;
    va_list ap2; va_copy(ap2, ap);
    for (int32_t i = 0; i < n; i++) total += strlen(va_arg(ap2, const char*));
    va_end(ap2);
    char* r = malloc(total + 1); size_t ri = 0;
    for (int32_t i = 0; i < n; i++) {
        const char* part = va_arg(ap, const char*);
        size_t pl = strlen(part);
        memcpy(r + ri, part, pl); ri += pl;
    }
    va_end(ap); r[ri] = '\0'; return cv_str(r);
}

/* ═══════════════════════════════════════════════════════════════════════
   TYPE CONVERSIONS
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_str_conv(CoolVal v) { return cv_str(cool_to_str(v)); }
CoolVal cool_int_conv(CoolVal v) {
    if (v.tag == TAG_INT)   return v;
    if (v.tag == TAG_FLOAT) return cv_int((int64_t)cv_as_float(v));
    if (v.tag == TAG_BOOL)  return cv_int(v.payload);
    if (v.tag == TAG_STR)   return cv_int(strtoll(as_cstr(v), NULL, 10));
    return cv_int(0);
}
CoolVal cool_float_conv(CoolVal v) {
    if (v.tag == TAG_FLOAT) return v;
    if (v.tag == TAG_INT)   return cv_float((double)v.payload);
    if (v.tag == TAG_BOOL)  return cv_float((double)v.payload);
    if (v.tag == TAG_STR)   return cv_float(strtod(as_cstr(v), NULL));
    return cv_float(0.0);
}

/* ═══════════════════════════════════════════════════════════════════════
   RANGE — returns a list
   ═══════════════════════════════════════════════════════════════════════ */
CoolVal cool_range1(CoolVal stop) {
    int64_t n = stop.payload;
    CoolVal list = cool_list_new();
    for (int64_t i = 0; i < n; i++) cool_list_append_raw(list, cv_int(i));
    return list;
}
CoolVal cool_range2(CoolVal start, CoolVal stop) {
    int64_t s = start.payload, e = stop.payload;
    CoolVal list = cool_list_new();
    for (int64_t i = s; i < e; i++) cool_list_append_raw(list, cv_int(i));
    return list;
}
CoolVal cool_range3(CoolVal start, CoolVal stop, CoolVal step) {
    int64_t s = start.payload, e = stop.payload, st = step.payload;
    CoolVal list = cool_list_new();
    if (st > 0) for (int64_t i = s; i < e; i += st) cool_list_append_raw(list, cv_int(i));
    else if (st < 0) for (int64_t i = s; i > e; i += st) cool_list_append_raw(list, cv_int(i));
    return list;
}

/* ═══════════════════════════════════════════════════════════════════════
   FILE I/O
   ═══════════════════════════════════════════════════════════════════════ */
typedef struct { FILE* fp; int closed; } CoolFile;
static CoolFile* cv_file_ptr(CoolVal v) { return (CoolFile*)(intptr_t)v.payload; }

CoolVal cool_file_open(CoolVal path, CoolVal mode) {
    const char* p = as_cstr(path);
    const char* m = (mode.tag == TAG_STR) ? as_cstr(mode) : "r";
    FILE* fp = fopen(p, m);
    if (!fp) { fprintf(stderr, "FileNotFoundError: '%s'\n", p); exit(1); }
    CoolFile* f = malloc(sizeof(CoolFile)); f->fp = fp; f->closed = 0;
    CoolVal v; v.tag = TAG_FILE; v.payload = (int64_t)(intptr_t)f; return v;
}
CoolVal cool_file_read(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (f->closed) { fputs("ValueError: I/O operation on closed file\n", stderr); exit(1); }
    fseek(f->fp, 0, SEEK_END); long size = ftell(f->fp); rewind(f->fp);
    char* buf = malloc((size_t)size + 1);
    size_t read = fread(buf, 1, (size_t)size, f->fp);
    buf[read] = '\0'; return cv_str(buf);
}
CoolVal cool_file_readline(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (f->closed) { fputs("ValueError: I/O operation on closed file\n", stderr); exit(1); }
    char* buf = malloc(4096); size_t cap = 4096, len = 0;
    int c;
    while ((c = fgetc(f->fp)) != EOF) {
        if (len + 2 >= cap) { cap *= 2; buf = realloc(buf, cap); }
        buf[len++] = (char)c;
        if (c == '\n') break;
    }
    buf[len] = '\0'; return cv_str(buf);
}
CoolVal cool_file_readlines(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (f->closed) { fputs("ValueError: I/O operation on closed file\n", stderr); exit(1); }
    CoolVal res = cool_list_new();
    char* buf = malloc(4096); size_t cap = 4096, len = 0;
    int c;
    while ((c = fgetc(f->fp)) != EOF) {
        if (len + 2 >= cap) { cap *= 2; buf = realloc(buf, cap); }
        buf[len++] = (char)c;
        if (c == '\n') {
            buf[len] = '\0';
            char* line = malloc(len + 1); memcpy(line, buf, len + 1);
            cool_list_append_raw(res, cv_str(line));
            len = 0;
        }
    }
    if (len > 0) {
        buf[len] = '\0';
        char* line = malloc(len + 1); memcpy(line, buf, len + 1);
        cool_list_append_raw(res, cv_str(line));
    }
    free(buf); return res;
}
CoolVal cool_file_write(CoolVal file, CoolVal text) {
    CoolFile* f = cv_file_ptr(file);
    if (f->closed) { fputs("ValueError: I/O operation on closed file\n", stderr); exit(1); }
    fputs(as_cstr(text), f->fp); return cv_nil();
}
CoolVal cool_file_writelines(CoolVal file, CoolVal lines) {
    CoolList* l = cv_list_ptr(lines);
    for (int64_t i = 0; i < l->len; i++) cool_file_write(file, l->data[i]);
    return cv_nil();
}
CoolVal cool_file_close(CoolVal file) {
    CoolFile* f = cv_file_ptr(file);
    if (!f->closed) { fclose(f->fp); f->closed = 1; }
    return cv_nil();
}

/* ═══════════════════════════════════════════════════════════════════════
   COOL_DISPATCH  — unified method call
   Dispatches on self.tag:  LIST / STR / DICT / INST
   ═══════════════════════════════════════════════════════════════════════ */
static CoolVal dispatch_on_inst(CoolVal self, int32_t cls, const char* name, int32_t argc, CoolVal* argv) {
    while (cls >= 0 && cls < MAX_CLASSES) {
        for (int32_t i = 0; i < g_n_methods; i++) {
            if (g_methods[i].class_id == cls && strcmp(g_methods[i].name, name) == 0)
                return g_methods[i].fn(self, argc, argv);
        }
        cls = g_parent_of[cls];
    }
    fprintf(stderr, "AttributeError: instance has no method '%s'\n", name); exit(1);
}

CoolVal cool_dispatch(CoolVal self, const char* name, int32_t argc, CoolVal* argv) {
    /* ── LIST methods ──────────────────────────────────────────────────── */
    if (self.tag == TAG_LIST) {
        CoolVal a0 = argc > 0 ? argv[0] : cv_nil();
        CoolVal a1 = argc > 1 ? argv[1] : cv_nil();
        if (!strcmp(name,"append"))  return cool_list_append(self, a0);
        if (!strcmp(name,"pop"))     return cool_list_pop(self, a0);
        if (!strcmp(name,"sort"))    return cool_list_sort(self, a0);
        if (!strcmp(name,"reverse")) return cool_list_reverse(self);
        if (!strcmp(name,"index"))   return cool_list_index(self, a0);
        if (!strcmp(name,"count"))   return cool_list_count(self, a0);
        if (!strcmp(name,"extend"))  return cool_list_extend(self, a0);
        if (!strcmp(name,"insert"))  return cool_list_insert(self, a0, a1);
        if (!strcmp(name,"remove"))  return cool_list_remove(self, a0);
        if (!strcmp(name,"clear"))   return cool_list_clear(self);
        if (!strcmp(name,"copy"))    return cool_list_copy(self);
        fprintf(stderr, "AttributeError: list has no method '%s'\n", name); exit(1);
    }
    /* ── STR methods ───────────────────────────────────────────────────── */
    if (self.tag == TAG_STR) {
        CoolVal a0 = argc > 0 ? argv[0] : cv_nil();
        CoolVal a1 = argc > 1 ? argv[1] : cv_nil();
        (void)a1;
        if (!strcmp(name,"upper"))      return cool_str_upper(self);
        if (!strcmp(name,"lower"))      return cool_str_lower(self);
        if (!strcmp(name,"strip"))      return cool_str_strip(self);
        if (!strcmp(name,"lstrip"))     return cool_str_lstrip(self);
        if (!strcmp(name,"rstrip"))     return cool_str_rstrip(self);
        if (!strcmp(name,"find"))       return cool_str_find(self, a0);
        if (!strcmp(name,"startswith")) return cool_str_startswith(self, a0);
        if (!strcmp(name,"endswith"))   return cool_str_endswith(self, a0);
        if (!strcmp(name,"replace"))    return cool_str_replace(self, a0, a1);
        if (!strcmp(name,"split"))      return cool_str_split(self, a0);
        if (!strcmp(name,"join"))       return cool_str_join(self, a0);
        if (!strcmp(name,"count"))      return cool_str_count(self, a0);
        if (!strcmp(name,"format"))     return cool_str_format(self, a0);
        if (!strcmp(name,"title"))      return cool_str_title(self);
        if (!strcmp(name,"capitalize")) return cool_str_capitalize(self);
        if (!strcmp(name,"isdigit"))    return cool_str_isdigit(self);
        if (!strcmp(name,"isalpha"))    return cool_str_isalpha(self);
        if (!strcmp(name,"isspace"))    return cool_str_isspace(self);
        fprintf(stderr, "AttributeError: str has no method '%s'\n", name); exit(1);
    }
    /* ── DICT methods ──────────────────────────────────────────────────── */
    if (self.tag == TAG_DICT) {
        CoolVal a0 = argc > 0 ? argv[0] : cv_nil();
        CoolVal a1 = argc > 1 ? argv[1] : cv_nil();
        if (!strcmp(name,"get"))    return cool_dict_get_default(self, a0, a1);
        if (!strcmp(name,"set"))    return cool_dict_set(self, a0, a1);
        if (!strcmp(name,"keys"))   return cool_dict_keys(self);
        if (!strcmp(name,"values")) return cool_dict_values(self);
        if (!strcmp(name,"items"))  return cool_dict_items(self);
        if (!strcmp(name,"pop"))    return cool_dict_pop(self, a0);
        if (!strcmp(name,"remove")) return cool_dict_remove(self, a0);
        if (!strcmp(name,"clear"))  return cool_dict_clear(self);
        if (!strcmp(name,"update")) return cool_dict_update(self, a0);
        if (!strcmp(name,"copy"))   return cool_dict_copy(self);
        fprintf(stderr, "AttributeError: dict has no method '%s'\n", name); exit(1);
    }
    /* ── FILE methods ──────────────────────────────────────────────────── */
    if (self.tag == TAG_FILE) {
        CoolVal a0 = argc > 0 ? argv[0] : cv_nil();
        if (!strcmp(name,"read"))       return cool_file_read(self);
        if (!strcmp(name,"readline"))   return cool_file_readline(self);
        if (!strcmp(name,"readlines"))  return cool_file_readlines(self);
        if (!strcmp(name,"write"))      return cool_file_write(self, a0);
        if (!strcmp(name,"writelines")) return cool_file_writelines(self, a0);
        if (!strcmp(name,"close"))      return cool_file_close(self);
        fprintf(stderr, "AttributeError: file has no method '%s'\n", name); exit(1);
    }
    /* ── INST methods ──────────────────────────────────────────────────── */
    if (self.tag == TAG_INST) {
        int32_t cls = cool_inst_class_id(self);
        return dispatch_on_inst(self, cls, name, argc, argv);
    }
    fprintf(stderr, "AttributeError: '%s' object has no method '%s'\n", tag_name(self.tag), name);
    exit(1);
}

/* cool_dispatch_super — call parent-class method directly */
CoolVal cool_dispatch_super(CoolVal self, int32_t parent_class_id, const char* name, int32_t argc, CoolVal* argv) {
    return dispatch_on_inst(self, parent_class_id, name, argc, argv);
}

/* ═══════════════════════════════════════════════════════════════════════
   EXTRA BUILTINS
   ═══════════════════════════════════════════════════════════════════════ */
/* sorted(list) → new sorted list */
CoolVal cool_sorted(CoolVal lst) {
    CoolVal copy = cool_list_copy(lst);
    cool_list_sort(copy, cv_nil());
    return copy;
}
/* reversed(list) → new reversed list */
CoolVal cool_reversed(CoolVal lst) {
    CoolVal copy = cool_list_copy(lst);
    cool_list_reverse(copy);
    return copy;
}
/* abs() */
CoolVal cool_abs(CoolVal v) {
    if (v.tag == TAG_FLOAT) return cv_float(fabs(cv_as_float(v)));
    return cv_int(v.payload < 0 ? -v.payload : v.payload);
}
/* min/max of a list */
CoolVal cool_min(CoolVal lst) {
    CoolList* l = cv_list_ptr(lst);
    if (l->len == 0) { fputs("ValueError: min() of empty list\n", stderr); exit(1); }
    CoolVal m = l->data[0];
    for (int64_t i = 1; i < l->len; i++) if (cmp_coolval(&l->data[i], &m) < 0) m = l->data[i];
    return m;
}
CoolVal cool_max(CoolVal lst) {
    CoolList* l = cv_list_ptr(lst);
    if (l->len == 0) { fputs("ValueError: max() of empty list\n", stderr); exit(1); }
    CoolVal m = l->data[0];
    for (int64_t i = 1; i < l->len; i++) if (cmp_coolval(&l->data[i], &m) > 0) m = l->data[i];
    return m;
}
CoolVal cool_sum(CoolVal lst) {
    CoolList* l = cv_list_ptr(lst);
    CoolVal acc = cv_int(0);
    for (int64_t i = 0; i < l->len; i++) acc = cool_add(acc, l->data[i]);
    return acc;
}
/* chr / ord */
CoolVal cool_chr(CoolVal v) {
    char* r = malloc(2); r[0] = (char)v.payload; r[1] = '\0'; return cv_str(r);
}
CoolVal cool_ord(CoolVal v) {
    return cv_int((int64_t)(unsigned char)as_cstr(v)[0]);
}

/* ═══════════════════════════════════════════════════════════════════════
   PRINT — handles all types
   ═══════════════════════════════════════════════════════════════════════ */
void cool_print(int32_t n, ...) {
    va_list ap; va_start(ap, n);
    for (int32_t i = 0; i < n; i++) {
        if (i > 0) putchar(' ');
        CoolVal v = va_arg(ap, CoolVal);
        fputs(cool_to_str(v), stdout);
    }
    va_end(ap); putchar('\n');
}

/* ═══════════════════════════════════════════════════════════════════════
   COMPAT WRAPPERS — old names / signatures used by the LLVM codegen
   ═══════════════════════════════════════════════════════════════════════ */

/* cool_list_make: codegen passes a CoolVal(int) as capacity hint — ignore it */
CoolVal cool_list_make(CoolVal cap_hint) { (void)cap_hint; return cool_list_new(); }
/* cool_list_push: old name for cool_list_append */
CoolVal cool_list_push(CoolVal list, CoolVal val) { return cool_list_append(list, val); }
/* cool_list_concat: concatenate two lists into a new one */
CoolVal cool_list_concat(CoolVal a, CoolVal b) {
    CoolVal res = cool_list_copy(a);
    cool_list_extend(res, b);
    return res;
}
/* cool_range: 3-arg wrapper */
CoolVal cool_range(CoolVal start, CoolVal stop, CoolVal step) {
    return cool_range3(start, stop, step);
}
/* cool_setitem: unified obj[key] = val for lists and dicts */
CoolVal cool_setitem(CoolVal obj, CoolVal key, CoolVal val) {
    if (obj.tag == TAG_LIST) return cool_list_set(obj, key, val);
    if (obj.tag == TAG_DICT) return cool_dict_set(obj, key, val);
    fprintf(stderr, "TypeError: object does not support item assignment\n"); exit(1);
}
/* cool_dispatch_v: variadic wrapper so codegen can call methods without argv array */
CoolVal cool_dispatch_v(CoolVal self, const char* name, int32_t argc, ...) {
    CoolVal argv[64];
    va_list ap; va_start(ap, argc);
    for (int32_t i = 0; i < argc && i < 64; i++) argv[i] = va_arg(ap, CoolVal);
    va_end(ap);
    return cool_dispatch(self, name, argc, argv);
}
