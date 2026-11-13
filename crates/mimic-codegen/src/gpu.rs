//! Fused maps as GPU kernels (Metal Shading Language).
//!
//! A fused map (fuse.rs) whose computation the GPU does *exactly* as the
//! CPU does is also written as a Metal kernel; the runtime (mimic_gpu.c)
//! runs that instead when the GPU is there and the work is large enough.
//! Apple GPUs have no f64, so a kernel is made only of
//!
//! - i64 and i32 integers (wrapping, as on the CPU) and truth values;
//! - f32 arithmetic: every float operation's operands are f32 values and its
//!   result is rounded to f32, which is exactly what computing in f64 and
//!   rounding gives for `+ - × ÷` (the CPU's way, see Node::Narrow).
//!
//! Anything else (f64 results, calls, records, division by a variable that
//! may be zero) gives no kernel, and the map runs on the CPU as before.

use super::fuse::Node;
use super::types::El;
use mimic_syntax::ast::{BinOp, UnOp};
use std::fmt::Write;

/// What the kernel reads from each slot of the plan.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Arg {
    /// unused (or a buffer)
    None,
    /// its 8 bytes as they are (an i64, a truth value, an index)
    Raw,
    /// an f64 handed over rounded to f32 (in the low 4 bytes)
    F32,
}

pub(crate) struct Kernel {
    pub src: String,
    /// estimated work per element (about one per simple operation)
    pub cost: i64,
    /// the leaves read as buffers, in buffer order: slot and element type
    pub bufs: Vec<(usize, El)>,
    /// per slot of the plan
    pub args: Vec<Arg>,
}

/// The type of a value in the kernel.
#[derive(Clone, Copy, PartialEq)]
enum G {
    /// i64
    L,
    /// f32, exact
    F,
    /// truth value
    B,
    /// an f64 the CPU would keep in f64: usable only rounded to f32 (the
    /// text is already that rounding)
    D,
    /// an f64 result of an operation on f32 values: usable only rounded
    Op,
}

struct Tr {
    bufs: Vec<(usize, El)>,
    args: Vec<Arg>,
    cost: i64,
}

impl Tr {
    fn buf(&mut self, slot: usize, el: El) -> usize {
        if let Some(k) = self.bufs.iter().position(|(s, _)| *s == slot) {
            return k;
        }
        self.bufs.push((slot, el));
        self.bufs.len() - 1
    }

    fn arg(&mut self, slot: usize, a: Arg) -> Option<()> {
        match self.args.get(slot)? {
            Arg::None => self.args[slot] = a,
            x if *x == a => {}
            _ => return None,
        }
        Some(())
    }

    /// The value of `n` at element `ix` (text), with its type.
    fn node(&mut self, n: &Node, ix: &str) -> Option<(String, G)> {
        Some(match n {
            Node::Index => (format!("({ix})"), G::L),
            Node::Offset(s) => {
                self.arg(*s, Arg::Raw)?;
                (format!("(a[{s}] + {ix})"), G::L)
            }
            Node::Leaf(s, el) => {
                let k = self.buf(*s, *el);
                match el {
                    El::F32 => (format!("b{k}[{ix}]"), G::F),
                    El::I32 | El::I64 => (format!("((long)b{k}[{ix}])"), G::L),
                    El::Bool => (format!("(b{k}[{ix}] != 0)"), G::B),
                    _ => return None,
                }
            }
            Node::Scalar(s, t) => match t {
                super::Ty::I64 => {
                    self.arg(*s, Arg::Raw)?;
                    (format!("a[{s}]"), G::L)
                }
                super::Ty::Bool => {
                    self.arg(*s, Arg::Raw)?;
                    (format!("(a[{s}] != 0)"), G::B)
                }
                super::Ty::F64 => {
                    self.arg(*s, Arg::F32)?;
                    (format!("as_type<float>((uint)a[{s}])"), G::D)
                }
                _ => return None,
            },
            Node::ConstI(v) => {
                if *v == i64::MIN {
                    return None;
                }
                (format!("({v}L)"), G::L)
            }
            Node::ConstF(v) => {
                let f = *v as f32;
                let text = format!("as_type<float>({:#010x}u)", f.to_bits());
                (text, if f as f64 == *v { G::F } else { G::D })
            }
            Node::Narrow(el, a) => {
                let (x, t) = self.node(a, ix)?;
                match (el, t) {
                    (_, G::B) => (x, G::B),
                    (El::F32, G::F | G::D | G::Op) => (x, G::F),
                    // (i64 → f64 → f32 on the CPU: the same for |x| < 2^53)
                    (El::F32, G::L) => (format!("((float){x})"), G::F),
                    (El::I32, G::L) => (format!("((long)(int){x})"), G::L),
                    _ => return None,
                }
            }
            Node::Un(op, a) => {
                let (x, t) = self.node(a, ix)?;
                self.cost += 1;
                match (op, t) {
                    (UnOp::Not, G::B) => (format!("(!{x})"), G::B),
                    (UnOp::Neg, G::L) => (format!("((long)(0ul - (ulong){x}))"), G::L),
                    (UnOp::Neg, G::F) => (format!("(-{x})"), G::F),
                    (UnOp::Abs, G::L) => (format!("((long)({x} < 0 ? 0ul - (ulong){x} : (ulong){x}))"), G::L),
                    (UnOp::Abs, G::F) => (format!("fabs({x})"), G::F),
                    (UnOp::BitNot, G::L) => (format!("(~{x})"), G::L),
                    _ => return None,
                }
            }
            Node::Bin(op, a, b) => {
                let (x, tx) = self.node(a, ix)?;
                let (y, ty) = self.node(b, ix)?;
                self.bin(*op, x, tx, y, ty, b)?
            }
            Node::Sel(c, a, b) => {
                let (c, tc) = self.node(c, ix)?;
                let (x, tx) = self.node(a, ix)?;
                let (y, ty) = self.node(b, ix)?;
                if tc != G::B || tx != ty || !matches!(tx, G::L | G::F | G::B) {
                    return None;
                }
                (format!("({c} ? {x} : {y})"), tx)
            }
            Node::Rev(s, a) => {
                self.arg(*s, Arg::Raw)?;
                self.node(a, &format!("(a[{s}] - ({ix}))"))?
            }
            Node::Shift(s, a) => {
                self.arg(*s, Arg::Raw)?;
                self.node(a, &format!("(({ix}) + a[{s}])"))?
            }
            Node::Apply { .. } | Node::LeafObj(_) => return None,
        })
    }

    fn bin(&mut self, op: BinOp, x: String, tx: G, y: String, ty: G, rhs: &Node) -> Option<(String, G)> {
        use BinOp::*;
        self.cost += 1;
        if matches!(op, And | Or) {
            if tx != G::B || ty != G::B {
                return None;
            }
            return Some((format!("({x} {} {y})", if op == And { "&&" } else { "||" }), G::B));
        }
        let cmp = match op {
            Eq => Some("=="),
            Ne => Some("!="),
            Lt => Some("<"),
            Le => Some("<="),
            Gt => Some(">"),
            Ge => Some(">="),
            _ => None,
        };
        let float = matches!(tx, G::F | G::D | G::Op) || matches!(ty, G::F | G::D | G::Op) || op == Div;
        if float {
            // operands exactly f32 (an integer only where it is: |x| ≤ 2^24
            // can't be known here, so no integers)
            if tx != G::F || ty != G::F {
                return None;
            }
            if let Some(c) = cmp {
                return Some((format!("({x} {c} {y})"), G::B));
            }
            return Some(match op {
                Add => (format!("({x} + {y})"), G::Op),
                Sub => (format!("({x} - {y})"), G::Op),
                Mul => (format!("({x} * {y})"), G::Op),
                Div => {
                    self.cost += 3;
                    (format!("({x} / {y})"), G::Op)
                }
                Max => (format!("({x} > {y} ? {x} : {y})"), G::F),
                Min => (format!("({x} < {y} ? {x} : {y})"), G::F),
                _ => return None,
            });
        }
        if tx == G::B || ty == G::B {
            // (truth values compare as truth values)
            return match (tx, ty, cmp) {
                (G::B, G::B, Some(c @ ("==" | "!="))) => Some((format!("({x} {c} {y})"), G::B)),
                _ => None,
            };
        }
        if let Some(c) = cmp {
            return Some((format!("({x} {c} {y})"), G::B));
        }
        let w = |o: &str| format!("((long)((ulong){x} {o} (ulong){y}))");
        Some(match op {
            Add => (w("+"), G::L),
            Sub => (w("-"), G::L),
            Mul => {
                self.cost += 1;
                (w("*"), G::L)
            }
            Max => (format!("max({x}, {y})"), G::L),
            Min => (format!("min({x}, {y})"), G::L),
            BitAnd => (format!("({x} & {y})"), G::L),
            BitOr => (format!("({x} | {y})"), G::L),
            BitXor => (format!("({x} ^ {y})"), G::L),
            // floored, by a constant that is neither 0 nor -1 (no faults)
            IntDiv | Mod => {
                let Node::ConstI(d) = rhs else { return None };
                if *d == 0 || *d == -1 {
                    return None;
                }
                self.cost += 8;
                let f = if op == IntDiv { "fdiv" } else { "fmod_" };
                (format!("{f}({x}, {y})"), G::L)
            }
            _ => return None,
        })
    }
}

/// The kernel for a fused map of `root` producing `out` elements, if the
/// GPU computes it exactly. `nslots`: the plan's slots.
pub(crate) fn map_kernel(root: &Node, out: El, nslots: usize) -> Option<Kernel> {
    let mut tr = Tr { bufs: Vec::new(), args: vec![Arg::None; nslots], cost: 0 };
    let (x, t) = tr.node(root, "i")?;
    let (oty, value) = match (out, t) {
        (El::F32, G::F) => ("float", x),
        (El::I32, G::L) => ("int", format!("(int){x}")),
        (El::I64, G::L) => ("long", x),
        (El::Bool, G::B) => ("uchar", format!("(uchar){x}")),
        _ => return None,
    };
    if tr.bufs.len() > 30 {
        return None;
    }
    let mut src = String::from(
        "#include <metal_stdlib>\nusing namespace metal;\n\
         static inline long fdiv(long x, long y) { long q = x / y; return q - ((x % y != 0) && ((x % y < 0) != (y < 0))); }\n\
         static inline long fmod_(long x, long y) { long r = x % y; return (r != 0 && ((r < 0) != (y < 0))) ? r + y : r; }\n\
         kernel void k(",
    );
    for (k, (_, el)) in tr.bufs.iter().enumerate() {
        let t = match el {
            El::F32 => "float",
            El::I32 => "int",
            El::Bool => "uchar",
            _ => "long",
        };
        let _ = write!(src, "device const {t} *b{k} [[buffer({k})]], ");
    }
    let nb = tr.bufs.len();
    let _ = write!(
        src,
        "device {oty} *out [[buffer({nb})]], device const long *a [[buffer({})]], uint gi [[thread_position_in_grid]]) {{\n    long i = (long)gi;\n    out[i] = {value};\n}}\n",
        nb + 1
    );
    Some(Kernel { src, cost: tr.cost.max(1), bufs: tr.bufs, args: tr.args })
}
