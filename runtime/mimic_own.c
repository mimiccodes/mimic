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
