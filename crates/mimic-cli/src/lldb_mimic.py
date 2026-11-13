# lldb formatters for Mimic values (loaded by `mimic lldb`).
#
# A Mimic value is a pointer to one block: a 32-byte header (reference
# count, element type, rank, length, record descriptor) and the elements.
# Debug information names such pointers `value`; this shows what they hold,
# the way Mimic prints it: 42, 2.5, true, "text", [1 2 3], Point { x: 1, y: 2 },
# Some(3).

import lldb
import struct

I64, F64, BOOL, CHAR, BOXED, OBJ, FN, MAP, U8, F32, I32 = range(11)
MAX_ITEMS = 12
MAX_DEPTH = 3
MAX_TEXT = 200


def _read(proc, addr, n):
    err = lldb.SBError()
    b = proc.ReadMemory(addr, n, err)
    if not err.Success() or b is None:
        raise ValueError("unreadable")
    return b


def _cstr(proc, addr):
    if addr == 0:
        return None
    err = lldb.SBError()
    s = proc.ReadCStringFromMemory(addr, 256, err)
    return s if err.Success() else None


def _num(dtype, raw):
    if dtype in (I64,):
        return str(struct.unpack("<q", raw)[0])
    if dtype == F64:
        x = struct.unpack("<d", raw)[0]
        return repr(x) if x != int(x) or abs(x) >= 1e16 else "%.1f" % x
    if dtype == F32:
        x = struct.unpack("<f", raw)[0]
        return repr(x)
    if dtype == I32:
        return str(struct.unpack("<i", raw)[0])
    if dtype == BOOL:
        return "true" if raw[0] else "false"
    if dtype == U8:
        return str(raw[0])
    return "?"


SIZE = {I64: 8, F64: 8, F32: 4, I32: 4, BOOL: 1, U8: 1, CHAR: 1}


def show(proc, addr, depth=0):
    if addr == 0:
        return "()"
    try:
        rc, dtype, rank, length, desc = struct.unpack("<qiiqQ", _read(proc, addr, 32))
    except ValueError:
        return "<unreadable 0x%x>" % addr
    data = addr + 32
    if dtype < 0 or dtype > I32 or length < 0:
        return "<not a Mimic value 0x%x>" % addr
    if dtype == CHAR:
        n = min(length, MAX_TEXT)
        text = _read(proc, data, n).decode("utf-8", "replace") if n else ""
        return '"%s%s"' % (text.replace('"', '\\"'), "…" if length > n else "")
    if dtype in SIZE and rank == 0:
        return _num(dtype, _read(proc, data, SIZE[dtype]))
    if depth >= MAX_DEPTH:
        return "…"
    if dtype in SIZE:
        n = min(length, MAX_ITEMS)
        s = SIZE[dtype]
        raw = _read(proc, data, n * s) if n else b""
        items = [_num(dtype, raw[i * s:(i + 1) * s]) for i in range(n)]
        if length > n:
            items.append("… (%d in all)" % length)
        return "[" + " ".join(items) + "]"
    if dtype == BOXED:
        n = min(length, MAX_ITEMS)
        ptrs = struct.unpack("<%dQ" % n, _read(proc, data, n * 8)) if n else ()
        items = [show(proc, p, depth + 1) for p in ptrs]
        if length > n:
            items.append("… (%d in all)" % length)
        return "[" + " ".join(items) + "]"
    if dtype == OBJ:
        if desc == 0:
            n = min(length, MAX_ITEMS)
            ptrs = struct.unpack("<%dQ" % n, _read(proc, data, n * 8)) if n else ()
            return "(" + ", ".join(show(proc, p, depth + 1) for p in ptrs) + ")"
        tname_p, variant_p, tag, nfields, fields_p, raw, fbits, bbits = struct.unpack("<QQiiQQQQ", _read(proc, desc, 56))
        tname = _cstr(proc, tname_p) or "?"
        variant = _cstr(proc, variant_p)
        n = min(length, MAX_ITEMS)
        slots = _read(proc, data, n * 8) if n else b""
        vals = []
        for i in range(n):
            word = slots[i * 8:(i + 1) * 8]
            if i < 64 and (raw >> i) & 1:
                if (fbits >> i) & 1:
                    vals.append(_num(F64, word))
                elif (bbits >> i) & 1:
                    vals.append(_num(BOOL, word))
                else:
                    vals.append(_num(I64, word))
            else:
                vals.append(show(proc, struct.unpack("<Q", word)[0], depth + 1))
        if variant is not None:
            return variant if not vals else "%s(%s)" % (variant, ", ".join(vals))
        names = []
        if fields_p:
            fptrs = struct.unpack("<%dQ" % n, _read(proc, fields_p, n * 8)) if n else ()
            names = [_cstr(proc, p) or "?" for p in fptrs]
        pairs = ["%s: %s" % (names[i] if i < len(names) else i, vals[i]) for i in range(n)]
        return "%s { %s }" % (tname, ", ".join(pairs))
    if dtype == FN:
        return "<function>"
    if dtype == MAP:
        return "<map of %d>" % length
    return "<value 0x%x>" % addr


def summary(valobj, _dict):
    try:
        return show(valobj.GetProcess(), valobj.GetValueAsUnsigned())
    except Exception as e:  # never break the debugger
        return "<%s>" % e


def __lldb_init_module(debugger, _dict):
    debugger.HandleCommand("type summary add -F %s.summary value" % __name__)
    debugger.HandleCommand("type format add -f decimal i64")
