/* Mimic runtime — array layer (Milestone 1).
 *
 * Every Mimic value is a refcounted, immutable-by-default array handle.
 * Scalars are rank-0 arrays. Compiled code follows one ownership rule:
 *   - runtime functions BORROW their arguments and return an OWNED (+1) value;
 *   - compiled Mimic functions borrow parameters and return an owned value.
 * The compiler inserts mim_retain/mim_release, so there is no GC and no
 * use-after-free. `mut` writes are copy-on-write (in place when rc == 1).
 *
 * Refcounts are atomic and bulk kernels run on a thread pool
 * (MIMIC_THREADS, MIMIC_PAR_GRAIN). The interaction-net layer holds ARR
 * handles as opaque nodes: DUP = retain, ERA = release.
 */
#ifndef MIMIC_RT_H
#define MIMIC_RT_H
#include <stdint.h>

typedef enum { MIM_I64 = 0, MIM_F64 = 1, MIM_BOOL = 2, MIM_CHAR = 3 } mim_dtype;

typedef struct mim_val {
    int64_t rc;
    int32_t dtype;
    int32_t rank;   /* 0 = scalar, 1 = vector (higher ranks: M4) */
    int64_t len;    /* element count */
    void *data;   /* points at inl: header and elements are one allocation */
    int64_t inl[];
} mim_val;

typedef mim_val *(*mim_fn1)(mim_val *);

/* lifecycle */
void mim_rt_init(int32_t argc, char **argv);
int32_t mim_rt_shutdown(int32_t leak_check);
void mim_retain(mim_val *v);
void mim_release(mim_val *v);
void mim_panic(const char *msg);

/* constructors */
mim_val *mim_int(int64_t x);
mim_val *mim_float(double x);
mim_val *mim_bool(int64_t x);
mim_val *mim_str(const char *s, int64_t len);
mim_val *mim_array_lit(int64_t n, mim_val **elems);
/* literals: created once (immortal) and cached in *slot */
mim_val *mim_lit_int(mim_val **slot, int64_t x);
mim_val *mim_lit_float(mim_val **slot, double x);
mim_val *mim_lit_str(mim_val **slot, const char *s, int64_t len);

/* operators: opcodes match mimic_syntax::ast::{BinOp, UnOp} */
mim_val *mim_binop(int32_t op, mim_val *a, mim_val *b);
mim_val *mim_unop(int32_t op, mim_val *a);
/* primitives: opcodes match mimic_syntax::glyphs::Prim */
mim_val *mim_prim1(int32_t prim, mim_val *a);
mim_val *mim_prim2(int32_t prim, mim_val *a, mim_val *b);
/* pure != 0: f has no side effects, so elements may be processed in parallel */
mim_val *mim_each(mim_val *xs, mim_fn1 f, int32_t pure);
mim_val *mim_index(mim_val *xs, mim_val *i);
void mim_set_index(mim_val **slot, mim_val *i, mim_val *v);
int64_t mim_truthy(mim_val *v);
void mim_check_type(mim_val *v, int32_t ty, const char *what);

/* FFI marshalling */
int64_t mim_as_i64(mim_val *v);
double mim_as_f64(mim_val *v);
const char *mim_cstr(mim_val *v);
void *mim_dataptr(mim_val *v);
mim_val *mim_from_cstr(const char *s);

#endif
