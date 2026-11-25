/* Lists and maps handed over by compiled code at their last use, changed in
 * place: `push`, `put` and `concat` of a value the compiled code owns there
 * (the compiler calls these instead of mim_std_push_own and the others).
 *
 * At the very end of the runtime: a function added earlier in it moved the
 * code of every function after it, which on some machines changed the speed
 * of programs that never call it. */

/* a list of values held by reference (strings, records, lists, …), held
 * once: its slots can change in place, each holding a reference of its own */
static int own_handles(const mim_val *xs) {
    return xs && xs->rank == 1 && xs->dtype == MIM_BOXED && __atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) == 1;
}

/* push(xs, x), xs owned: a list of references held once grows in place
 * (before, a list of strings or records was copied at every push) */
mim_val *mim_take_push(mim_val *xs, mim_val *x) {
    if (own_handles(xs) && x && x != xs) {
        int64_t n = xs->len;
        xs = grow_owned(xs, n + 1);
        mim_retain(x);
        ((mim_val **)xs->data)[n] = x;
        return xs;
    }
    return mim_std_push_own(xs, x);
}

/* put(xs, i, v), xs owned: a list of references held once, or a map held
 * once (as `m[k] = v` changes one), changed in place */
mim_val *mim_take_put(mim_val *xs, mim_val *k, mim_val *v) {
    if (own_handles(xs) && v && v != xs && k && k->rank == 0 && (k->dtype == MIM_I64 || k->dtype == MIM_I32)) {
        int64_t i = geti(k, 0);
        if (i >= 0 && i < xs->len) {
            mim_val **slot = (mim_val **)xs->data + i;
            mim_val *old = *slot;
            mim_retain(v);
            *slot = v;
            mim_release(old);
            return xs;
        }
    }
    if (xs && xs->dtype == MIM_MAP && v != xs && __atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) == 1) {
        mim_val *slot = xs;
        mim_set_index(&slot, k, v);
        return slot;
    }
    return mim_std_put_own(xs, k, v);
}

/* concat(xs, ys), xs owned */
mim_val *mim_take_concat(mim_val *xs, mim_val *ys) {
    return mim_std_concat_own(xs, ys);
}

/* xs[i] = v on a variable's list: the list (owned) in, the list out (a
 * copy when it was shared), so the compiled code keeps the variable in a
 * register instead of handing the runtime its address */
mim_val *mim_set_index_v(mim_val *xs, mim_val *i, mim_val *v) {
    mim_val *slot = xs;
    mim_set_index(&slot, i, v);
    return slot;
}

/* ==== a float's bits, and floats as bytes ================================
 * (math::to_bits and the rest, bytes::of_f64s and bytes::to_f64s: weights
 * saved exactly, as 8 bytes each, little-endian on every target) */

static int64_t bits_int_arg(mim_val *x, const char *fn) {
    if (!x || x->rank != 0 || IS_FLOAT(x->dtype) || x->dtype == MIM_BOOL || IS_HANDLES(x)) {
        char buf[96];
        snprintf(buf, sizeof buf, "%s: expected an integer", fn);
        mim_panic(buf);
    }
    return geti(x, 0);
}

mim_val *mim_std_math_to_bits(mim_val *a) {
    double x = math_arg(a, "to_bits");
    uint64_t u;
    memcpy(&u, &x, 8);
    return mim_int((int64_t)u);
}

mim_val *mim_std_math_from_bits(mim_val *b) {
    uint64_t u = (uint64_t)bits_int_arg(b, "math::from_bits");
    double x;
    memcpy(&x, &u, 8);
    return mim_float(x);
}

mim_val *mim_std_math_to_bits32(mim_val *a) {
    float x = (float)math_arg(a, "to_bits32");
    uint32_t u;
    memcpy(&u, &x, 4);
    return mim_int((int64_t)u);
}

mim_val *mim_std_math_from_bits32(mim_val *b) {
    int64_t i = bits_int_arg(b, "math::from_bits32");
    if (i < 0 || i > 0xFFFFFFFFLL) mim_panic("math::from_bits32: the bits of an f32 are 0 … 4294967295");
    uint32_t u = (uint32_t)i;
    float x;
    memcpy(&x, &u, 4);
    return mim_float32(x);
}

mim_val *mim_std_bytes_of_f64s(mim_val *xs) {
    need(xs, "bytes::of_f64s");
    if (xs->rank != 1 || IS_HANDLES(xs) || (xs->len > 0 && !IS_FLOAT(xs->dtype) && xs->dtype != MIM_I64))
        mim_panic("bytes::of_f64s: expected a list of floats");
    mim_val *r = alloc(MIM_U8, 1, xs->len * 8);
    uint8_t *o = (uint8_t *)r->data;
    for (int64_t i = 0; i < xs->len; i++) {
        double x = getf(xs, i);
        uint64_t u;
        memcpy(&u, &x, 8);
        for (int k = 0; k < 8; k++) o[i * 8 + k] = (uint8_t)(u >> (8 * k));
    }
    return r;
}

mim_val *mim_std_bytes_to_f64s(mim_val *b) {
    need(b, "bytes::to_f64s");
    if (b->rank != 1 || (b->len > 0 && b->dtype != MIM_U8)) mim_panic("bytes::to_f64s: expected bytes ([u8])");
    if (b->len % 8) {
        char buf[128];
        snprintf(buf, sizeof buf, "bytes::to_f64s: %lld bytes is not a whole number of floats (8 bytes each)", (long long)b->len);
        mim_panic(buf);
    }
    mim_val *r = alloc(MIM_F64, 1, b->len / 8);
    const uint8_t *d = (const uint8_t *)b->data;
    for (int64_t i = 0; i < b->len / 8; i++) {
        uint64_t u = 0;
        for (int k = 0; k < 8; k++) u |= (uint64_t)d[i * 8 + k] << (8 * k);
        double x;
        memcpy(&x, &u, 8);
        ((double *)r->data)[i] = x;
    }
    return r;
}

/* An f64 as text: the shortest that reads back as the same number (15
 * significant digits where those do, as before; 16 or 17 where they do not:
 * "{x}" then parses back to x, as in Rust, Go, JavaScript and Python) */
static void fmt_f64(char *buf, size_t n, double x) {
    for (int prec = 15; prec <= 17; prec++) {
        snprintf(buf, n, "%.*g", prec, x);
        if (!isfinite(x) || strtod(buf, NULL) == x) return;
    }
}
