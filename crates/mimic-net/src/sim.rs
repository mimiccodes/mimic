//! Nets as graphs, and reduction step by step: what `mimic net show` and
//! `mimic net trace` print, for editors to draw.
//!
//! A [`Graph`] is a definition laid out as nodes with ports (0 is the
//! principal port, 1 and 2 the auxiliary ones) and the wires between them;
//! a wire between two principal ports is an active pair (a redex).
//!
//! [`trace`] reduces a net with the same interaction rules as the runtime
//! (runtime/mimic_net.c: annihilate, commute, erase, copy, operate, switch,
//! call), but in rounds: every redex ready at the start of a round fires in
//! that round, as if each had a core of its own. The number of redexes per
//! round is the parallelism the net offers. It is a model: the runtime also
//! runs some calls natively and has a fixed number of cores. Numbers only:
//! nets that compute on arrays are not traced.

use crate::{Book, Def, Num, Op, Tree};
use mimic_syntax::ast::BinOp;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Where the definition's result comes out (not a node of the net).
    Root,
    Con,
    Dup(u32),
    Era,
    Num(Num),
    Ref(String),
    /// An operator; `flip` once it holds its first operand on port 1.
    Opr(Op, bool),
    Swi,
}

impl Kind {
    pub fn binary(&self) -> bool {
        matches!(self, Kind::Con | Kind::Dup(_) | Kind::Opr(..) | Kind::Swi)
    }

    /// `con`, `dup`, `era`, `num`, `ref`, `opr`, `swi`, `root`.
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Root => "root",
            Kind::Con => "con",
            Kind::Dup(_) => "dup",
            Kind::Era => "era",
            Kind::Num(_) => "num",
            Kind::Ref(_) => "ref",
            Kind::Opr(..) => "opr",
            Kind::Swi => "swi",
        }
    }

    /// As the `.mnet` text writes it: `( )`, `{ }`, `*`, `#5`, `@fib`, `$+`, `?`.
    pub fn label(&self) -> String {
        match self {
            Kind::Root => "result".into(),
            Kind::Con => "( )".into(),
            Kind::Dup(0) => "{ }".into(),
            Kind::Dup(l) => format!("{{{l}}}"),
            Kind::Era => "*".into(),
            Kind::Num(n) => format!("#{n}"),
            Kind::Ref(r) => format!("@{r}"),
            Kind::Opr(op, _) => format!("${}", op.name()),
            Kind::Swi => "?".into(),
        }
    }
}

type PortRef = (usize, u8);

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    /// The definition the node was made from (for linking to the source).
    pub def: String,
    pub ports: [Option<PortRef>; 3],
}

/// A net: nodes (freed ones are `None`) and their wires.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Option<Node>>,
    /// The root's node, if the graph has one.
    pub root: Option<usize>,
}

/// One end of a connection while a tree is laid out: a port, or a variable
/// whose other occurrence is not placed yet.
enum End {
    Port(PortRef),
    Var(String),
}

impl Graph {
    fn add(&mut self, kind: Kind, def: &str) -> usize {
        self.nodes.push(Some(Node { kind, def: def.to_string(), ports: [None; 3] }));
        self.nodes.len() - 1
    }

    pub fn node(&self, i: usize) -> &Node {
        self.nodes[i].as_ref().expect("a live node")
    }

    fn wire(&mut self, a: PortRef, b: PortRef) {
        if let Some(n) = self.nodes[a.0].as_mut() {
            n.ports[a.1 as usize] = Some(b);
        }
        if let Some(n) = self.nodes[b.0].as_mut() {
            n.ports[b.1 as usize] = Some(a);
        }
    }

    fn peer(&self, p: PortRef) -> Option<PortRef> {
        self.nodes[p.0].as_ref().and_then(|n| n.ports[p.1 as usize])
    }

    /// Is `p` (a principal port) part of an active pair?
    pub fn active(&self, i: usize) -> Option<usize> {
        let n = self.nodes[i].as_ref()?;
        if n.kind == Kind::Root {
            return None;
        }
        match n.ports[0] {
            Some((j, 0)) if self.nodes[j].as_ref().is_some_and(|m| m.kind != Kind::Root) => Some(j),
            _ => None,
        }
    }

    /// Every active pair once (lower node first).
    pub fn redexes(&self) -> Vec<(usize, usize)> {
        (0..self.nodes.len()).filter_map(|i| self.active(i).filter(|j| i < *j).map(|j| (i, j))).collect()
    }

    pub fn live(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }

    /// Lay out `t`; returns the end its principal port is (or a variable).
    fn tree(&mut self, t: &Tree, def: &str, vars: &mut HashMap<String, End>) -> End {
        let bin = |g: &mut Graph, kind: Kind, a: &Tree, b: &Tree, vars: &mut HashMap<String, End>| {
            let n = g.add(kind, def);
            let ea = g.tree(a, def, vars);
            g.link(End::Port((n, 1)), ea, vars);
            let eb = g.tree(b, def, vars);
            g.link(End::Port((n, 2)), eb, vars);
            End::Port((n, 0))
        };
        match t {
            Tree::Var(v) => End::Var(v.clone()),
            Tree::Era => End::Port((self.add(Kind::Era, def), 0)),
            Tree::Num(n) => End::Port((self.add(Kind::Num(*n), def), 0)),
            Tree::Ref(r) => End::Port((self.add(Kind::Ref(r.clone()), def), 0)),
            Tree::Con(a, b) => bin(self, Kind::Con, a, b, vars),
            Tree::Dup(l, a, b) => bin(self, Kind::Dup(*l), a, b, vars),
            Tree::Opr(op, a, b) => bin(self, Kind::Opr(*op, false), a, b, vars),
            Tree::Swi(a, b) => bin(self, Kind::Swi, a, b, vars),
        }
    }

    fn link(&mut self, a: End, b: End, vars: &mut HashMap<String, End>) {
        match (a, b) {
            (End::Port(p), End::Port(q)) => self.wire(p, q),
            (End::Var(x), e) | (e, End::Var(x)) => match vars.remove(&x) {
                Some(other) => self.link(other, e, vars),
                None => {
                    vars.insert(x, e);
                }
            },
        }
    }

    /// Lay out `d` with its root connected to `at`.
    fn instantiate(&mut self, d: &Def, at: PortRef) {
        let mut vars = HashMap::new();
        let r = self.tree(&d.root, &d.name, &mut vars);
        self.link(End::Port(at), r, &mut vars);
        for (a, b) in &d.redexes {
            let ea = self.tree(a, &d.name, &mut vars);
            let eb = self.tree(b, &d.name, &mut vars);
            self.link(ea, eb, &mut vars);
        }
    }

    /// A definition on its own: its nodes, with a root node for its result.
    pub fn of_def(d: &Def) -> Graph {
        let mut g = Graph::default();
        let root = g.add(Kind::Root, &d.name);
        g.root = Some(root);
        g.instantiate(d, (root, 0));
        g
    }
}

/// One round of reduction.
#[derive(Clone, Debug, Default)]
pub struct Step {
    /// Redexes that fired together.
    pub redexes: usize,
    /// How many of each rule (`annihilate`, `commute`, `erase`, `copy`,
    /// `operate`, `switch`, `call`, `void`).
    pub rules: HashMap<&'static str, usize>,
    /// Nodes alive after the round.
    pub nodes: usize,
}

pub struct Trace {
    pub steps: Vec<Step>,
    pub interactions: usize,
    /// The result, when it is a number.
    pub result: Option<Num>,
    /// Why it stopped early, if it did.
    pub stopped: Option<String>,
    /// The graph before each round, for small nets (see `frames`).
    pub frames: Vec<Graph>,
}

pub struct Limits {
    pub steps: usize,
    pub nodes: usize,
    /// Keep a frame per round while the net has at most this many nodes
    /// (0: none).
    pub frame_nodes: usize,
    pub frames: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { steps: 100_000, nodes: 2_000_000, frame_nodes: 0, frames: 0 }
    }
}

fn num_f(n: Num) -> f64 {
    match n {
        Num::Int(i) => i as f64,
        Num::F64(f) => f,
        Num::Bool(b) => b as i64 as f64,
    }
}

fn num_i(n: Num) -> i64 {
    match n {
        Num::Int(i) => i,
        Num::F64(f) => f as i64,
        Num::Bool(b) => b as i64,
    }
}

fn floormod(x: i64, y: i64) -> i64 {
    let r = x.wrapping_rem(y);
    if r != 0 && ((r ^ y) < 0) { r + y } else { r }
}

fn floordiv(x: i64, y: i64) -> i64 {
    let q = x.wrapping_div(y);
    if x.wrapping_rem(y) != 0 && ((x < 0) != (y < 0)) { q - 1 } else { q }
}

/// `a op b` on numbers, as the runtime's scalar_binop computes it.
fn binop(op: BinOp, a: Num, b: Num) -> Result<Num, String> {
    use BinOp::*;
    let bits = matches!(op, BitAnd | BitOr | BitXor | Shl | Shr | UShr);
    if bits {
        if matches!(a, Num::F64(_)) || matches!(b, Num::F64(_)) {
            return Err("bitwise operators need integers".into());
        }
        let (x, y) = (num_i(a), num_i(b));
        let both_bool = matches!((a, b), (Num::Bool(_), Num::Bool(_)));
        let v = match op {
            BitAnd => x & y,
            BitOr => x | y,
            BitXor => x ^ y,
            Shl => ((x as u64) << (y & 63)) as i64,
            Shr => x >> (y & 63),
            _ => ((x as u64) >> (y & 63)) as i64,
        };
        return Ok(if both_bool && matches!(op, BitAnd | BitOr | BitXor) { Num::Bool(v != 0) } else { Num::Int(v) });
    }
    if matches!(a, Num::F64(_)) || matches!(b, Num::F64(_)) || op == Div {
        let (x, y) = (num_f(a), num_f(b));
        return Ok(match op {
            Add => Num::F64(x + y),
            Sub => Num::F64(x - y),
            Mul => Num::F64(x * y),
            Div => Num::F64(x / y),
            Mod => Num::F64(x % y),
            Eq => Num::Bool(x == y),
            Ne => Num::Bool(x != y),
            Lt => Num::Bool(x < y),
            Le => Num::Bool(x <= y),
            Gt => Num::Bool(x > y),
            Ge => Num::Bool(x >= y),
            And => Num::Bool(x != 0.0 && y != 0.0),
            Or => Num::Bool(x != 0.0 || y != 0.0),
            IntDiv => Num::F64((x / y).floor()),
            Max => Num::F64(if x > y { x } else { y }),
            Min => Num::F64(if x < y { x } else { y }),
            _ => return Err(format!("`{}` is not traced", Op::Bin(op).name())),
        });
    }
    let (x, y) = (num_i(a), num_i(b));
    Ok(match op {
        Add => Num::Int(x.wrapping_add(y)),
        Sub => Num::Int(x.wrapping_sub(y)),
        Mul => Num::Int(x.wrapping_mul(y)),
        Mod if y == 0 => return Err("integer modulo by zero".into()),
        Mod => Num::Int(floormod(x, y)),
        IntDiv if y == 0 => return Err("integer division by zero".into()),
        IntDiv => Num::Int(floordiv(x, y)),
        Eq => Num::Bool(x == y),
        Ne => Num::Bool(x != y),
        Lt => Num::Bool(x < y),
        Le => Num::Bool(x <= y),
        Gt => Num::Bool(x > y),
        Ge => Num::Bool(x >= y),
        And => Num::Bool(x != 0 && y != 0),
        Or => Num::Bool(x != 0 || y != 0),
        Max => Num::Int(x.max(y)),
        Min => Num::Int(x.min(y)),
        _ => return Err(format!("`{}` is not traced", Op::Bin(op).name())),
    })
}

fn unop(op: Op, a: Num) -> Result<Num, String> {
    Ok(match (op, a) {
        (Op::Neg, Num::F64(f)) => Num::F64(-f),
        (Op::Neg, n) => Num::Int(num_i(n).wrapping_neg()),
        (Op::Not, n) => Num::Bool(num_f(n) == 0.0),
        (Op::Abs, Num::F64(f)) => Num::F64(f.abs()),
        (Op::Abs, n) => Num::Int(num_i(n).wrapping_abs()),
        (Op::BitNot, Num::Bool(b)) => Num::Bool(!b),
        (Op::BitNot, Num::Int(i)) => Num::Int(!i),
        // a type check passes the value through
        (Op::Check(..), n) => n,
        (op, _) => return Err(format!("`{}` works on arrays: nets with arrays are not traced", op.name())),
    })
}

struct Sim<'a> {
    g: Graph,
    defs: HashMap<&'a str, &'a Def>,
}

/// A rule rewires through the ports of the nodes it consumes, one wire at
/// a time, and frees them only at the end: when two of those ports are
/// wired to each other (`@id = (x x)`), a later step then finds the right
/// neighbour.
impl Sim<'_> {
    /// Join what is on the other side of ports `p` and `q`.
    fn fuse(&mut self, p: PortRef, q: PortRef) {
        if let (Some(x), Some(y)) = (self.g.peer(p), self.g.peer(q)) {
            self.g.wire(x, y);
        }
    }

    /// Put port `n` where port `p` was connected.
    fn attach(&mut self, n: PortRef, p: PortRef) {
        if let Some(x) = self.g.peer(p) {
            self.g.wire(n, x);
        }
    }

    /// A new nullary node where `p` was connected.
    fn put(&mut self, p: PortRef, kind: Kind, def: &str) {
        let n = self.g.add(kind, def);
        self.attach((n, 0), p);
    }

    /// The reference `a` facing the root `root`: expand it there.
    fn call_at_root(&mut self, a: usize, root: usize, rules: &mut HashMap<&'static str, usize>) -> Result<(), String> {
        let Kind::Ref(name) = self.g.node(a).kind.clone() else { return Ok(()) };
        *rules.entry("call").or_default() += 1;
        let d = *self.defs.get(name.as_str()).ok_or_else(|| format!("no definition @{name}"))?;
        self.free(&[a]);
        self.g.instantiate(d, (root, 0));
        Ok(())
    }

    fn free(&mut self, nodes: &[usize]) {
        for &n in nodes {
            self.g.nodes[n] = None;
        }
    }

    fn interact(&mut self, mut a: usize, mut b: usize, rules: &mut HashMap<&'static str, usize>) -> Result<(), String> {
        let mut rule = |r: &'static str| *rules.entry(r).or_default() += 1;
        if self.g.node(a).kind.binary() && !self.g.node(b).kind.binary() {
            std::mem::swap(&mut a, &mut b);
        }
        let (ka, kb) = (self.g.node(a).kind.clone(), self.g.node(b).kind.clone());
        let def_b = self.g.node(b).def.clone();
        if !ka.binary() && !kb.binary() {
            rule("void");
            self.free(&[a, b]);
            return Ok(());
        }
        match ka {
            Kind::Ref(name) => {
                if let Kind::Dup(_) = kb {
                    rule("copy");
                    self.put((b, 1), Kind::Ref(name.clone()), &def_b);
                    self.put((b, 2), Kind::Ref(name), &def_b);
                    self.free(&[a, b]);
                    return Ok(());
                }
                rule("call");
                let d = *self.defs.get(name.as_str()).ok_or_else(|| format!("no definition @{name}"))?;
                self.free(&[a]);
                self.g.instantiate(d, (b, 0));
                Ok(())
            }
            Kind::Era => {
                rule("erase");
                self.put((b, 1), Kind::Era, &def_b);
                self.put((b, 2), Kind::Era, &def_b);
                self.free(&[a, b]);
                Ok(())
            }
            Kind::Num(n) => match kb {
                Kind::Dup(_) => {
                    rule("copy");
                    self.put((b, 1), Kind::Num(n), &def_b);
                    self.put((b, 2), Kind::Num(n), &def_b);
                    self.free(&[a, b]);
                    Ok(())
                }
                Kind::Opr(op, flip) => {
                    rule("operate");
                    if op.is_unary() {
                        let v = unop(op, n)?;
                        self.put((b, 1), Kind::Era, &def_b);
                        self.put((b, 2), Kind::Num(v), &def_b);
                        self.free(&[a, b]);
                        return Ok(());
                    }
                    let Op::Bin(bop) = op else { return Err(format!("`{}` works on arrays: nets with arrays are not traced", op.name())) };
                    // the other operand, if it is a number already
                    let other = self.g.peer((b, 1)).and_then(|(j, p)| match &self.g.nodes[j] {
                        Some(Node { kind: Kind::Num(m), .. }) if p == 0 && j != a => Some((j, *m)),
                        _ => None,
                    });
                    match other {
                        Some((j, m)) => {
                            let v = if flip { binop(bop, m, n)? } else { binop(bop, n, m)? };
                            self.put((b, 2), Kind::Num(v), &def_b);
                            self.free(&[a, b, j]);
                        }
                        None => {
                            // wait for the other operand, holding this one
                            let o = self.g.add(Kind::Opr(op, !flip), &def_b);
                            let held = self.g.add(Kind::Num(n), &def_b);
                            self.g.wire((o, 1), (held, 0));
                            self.attach((o, 2), (b, 2));
                            self.attach((o, 0), (b, 1));
                            self.free(&[a, b]);
                        }
                    }
                    Ok(())
                }
                Kind::Swi => {
                    rule("switch");
                    let truth = num_f(n) != 0.0;
                    let sel = self.g.peer((b, 1));
                    match sel {
                        Some((j, 0)) if j != b && self.g.nodes[j].as_ref().is_some_and(|m| m.kind == Kind::Con) => {
                            let (keep, drop) = if truth { ((j, 1), (j, 2)) } else { ((j, 2), (j, 1)) };
                            self.fuse(keep, (b, 2));
                            self.put(drop, Kind::Era, &def_b);
                            self.free(&[a, b, j]);
                        }
                        _ => {
                            let c = self.g.add(Kind::Con, &def_b);
                            let e = self.g.add(Kind::Era, &def_b);
                            let (take_p, era_p) = if truth { ((c, 1), (c, 2)) } else { ((c, 2), (c, 1)) };
                            self.attach(take_p, (b, 2));
                            self.g.wire(era_p, (e, 0));
                            self.attach((c, 0), (b, 1));
                            self.free(&[a, b]);
                        }
                    }
                    Ok(())
                }
                _ => Err("a number met a function node (a type error in the net)".into()),
            },
            _ => {
                // two binary nodes
                let same = ka == kb && matches!(ka, Kind::Con | Kind::Dup(_));
                let def_a = self.g.node(a).def.clone();
                if same {
                    rule("annihilate");
                    self.fuse((a, 1), (b, 1));
                    self.fuse((a, 2), (b, 2));
                    self.free(&[a, b]);
                    return Ok(());
                }
                rule("commute");
                let n0 = self.g.add(kb.clone(), &def_b);
                let n1 = self.g.add(kb, &def_b);
                let n2 = self.g.add(ka.clone(), &def_a);
                let n3 = self.g.add(ka, &def_a);
                self.g.wire((n0, 1), (n2, 1));
                self.g.wire((n0, 2), (n3, 1));
                self.g.wire((n1, 1), (n2, 2));
                self.g.wire((n1, 2), (n3, 2));
                self.attach((n0, 0), (a, 1));
                self.attach((n1, 0), (a, 2));
                self.attach((n2, 0), (b, 1));
                self.attach((n3, 0), (b, 2));
                self.free(&[a, b]);
                Ok(())
            }
        }
    }
}

/// Reduce `entry` of `book` round by round.
pub fn trace(book: &Book, entry: &str, limits: &Limits) -> Result<Trace, String> {
    let defs: HashMap<&str, &Def> = book.defs.iter().map(|d| (d.name.as_str(), d)).collect();
    let d = *defs.get(entry).ok_or_else(|| format!("no definition @{entry}"))?;
    let mut sim = Sim { g: Graph::of_def(d), defs };
    let mut t = Trace { steps: Vec::new(), interactions: 0, result: None, stopped: None, frames: Vec::new() };
    loop {
        let mut redexes = sim.g.redexes();
        if redexes.is_empty() {
            // a reference as the result is expanded to read it, as the
            // runtime does (`@main = @yes`)
            let at_root = sim.g.root.and_then(|r| sim.g.peer((r, 0))).filter(|(j, p)| *p == 0 && matches!(sim.g.nodes[*j].as_ref().map(|n| &n.kind), Some(Kind::Ref(_))));
            match (at_root, sim.g.root) {
                (Some((j, _)), Some(root)) => redexes = vec![(j, root)],
                _ => break,
            }
        }
        if limits.frame_nodes > 0 && t.frames.len() < limits.frames && sim.g.live() <= limits.frame_nodes {
            t.frames.push(sim.g.clone());
        }
        if t.steps.len() >= limits.steps {
            t.stopped = Some(format!("stopped after {} rounds", limits.steps));
            break;
        }
        let mut step = Step { redexes: redexes.len(), ..Step::default() };
        for (a, b) in redexes {
            // an earlier redex of this round may have used these nodes
            let to_root = Some(b) == sim.g.root;
            if sim.g.nodes[a].is_none() || sim.g.nodes[b].is_none() || (!to_root && sim.g.active(a) != Some(b)) {
                step.redexes -= 1;
                continue;
            }
            let done = if to_root { sim.call_at_root(a, b, &mut step.rules) } else { sim.interact(a, b, &mut step.rules) };
            if let Err(e) = done {
                t.stopped = Some(e);
                break;
            }
            t.interactions += 1;
        }
        step.nodes = sim.g.live();
        t.steps.push(step);
        if t.stopped.is_some() {
            break;
        }
        if sim.g.live() > limits.nodes {
            t.stopped = Some(format!("stopped at {} nodes", limits.nodes));
            break;
        }
        // keep the node table from growing without end
        if sim.g.nodes.len() > 4 * sim.g.live() + 1024 {
            compact(&mut sim.g);
        }
    }
    if limits.frame_nodes > 0 && t.frames.len() < limits.frames && sim.g.live() <= limits.frame_nodes {
        t.frames.push(sim.g.clone());
    }
    if let Some(root) = sim.g.root
        && let Some((j, 0)) = sim.g.peer((root, 0))
        && let Some(Node { kind: Kind::Num(n), .. }) = &sim.g.nodes[j]
    {
        t.result = Some(*n);
    }
    Ok(t)
}

/// Drop freed slots, renumbering the nodes.
fn compact(g: &mut Graph) {
    let mut map = vec![usize::MAX; g.nodes.len()];
    let mut out = Vec::with_capacity(g.live());
    for (i, n) in g.nodes.iter().enumerate() {
        if n.is_some() {
            map[i] = out.len();
            out.push(n.clone());
        }
    }
    for n in out.iter_mut().flatten() {
        for p in n.ports.iter_mut().flatten() {
            p.0 = map[p.0];
        }
    }
    g.root = g.root.map(|r| map[r]);
    g.nodes = out;
}

/// `@main = r & @f ~ (#a (#b … r))`: a call of `f` on numbers.
pub fn call(f: &str, args: &[Num]) -> Def {
    let mut spine = Tree::Var("r".into());
    for a in args.iter().rev() {
        spine = Tree::Con(Box::new(Tree::Num(*a)), Box::new(spine));
    }
    Def { name: "main".into(), root: Tree::Var("r".into()), redexes: vec![(Tree::Ref(f.into()), spine)] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Trace {
        let book = crate::parse(src).unwrap();
        trace(&book, "main", &Limits::default()).unwrap()
    }

    #[test]
    fn fib_is_reduced_with_the_runtime_rules() {
        let t = run(include_str!("../../../tests/nets/fib.mnet"));
        assert_eq!(t.result, Some(Num::Int(6765)), "fib(20)");
        assert!(t.stopped.is_none());
        // fib's two calls run at the same time: rounds with many redexes
        let widest = t.steps.iter().map(|s| s.redexes).max().unwrap();
        assert!(widest > 100, "{widest}");
        assert!(t.steps.len() < t.interactions / 10, "rounds {} for {} interactions", t.steps.len(), t.interactions);
    }

    /// Every net fixture on numbers gives what the runtime prints.
    #[test]
    fn the_fixtures_agree_with_the_runtime() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/nets");
        for name in ["fib", "church", "lazy_branch", "numbers", "operand_order"] {
            let src = std::fs::read_to_string(dir.join(format!("{name}.mnet"))).unwrap();
            let want = std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap();
            let t = trace(&crate::parse(&src).unwrap(), "main", &Limits::default()).unwrap();
            assert!(t.stopped.is_none(), "{name}: {:?}", t.stopped);
            let got = match t.result {
                Some(Num::F64(f)) => format!("{f}"),
                Some(n) => format!("{n}").trim_start_matches('#').to_string(),
                None => "?".into(),
            };
            assert_eq!(got.trim(), want.trim(), "{name}");
        }
    }

    #[test]
    fn operand_order_and_numbers() {
        let book = crate::parse("@main = r & #10 ~ $-(#3 r)").unwrap();
        assert_eq!(trace(&book, "main", &Limits::default()).unwrap().result, Some(Num::Int(7)));
        let book = crate::parse("@main = r & a ~ $-(b r) & #10 ~ a & #3 ~ b").unwrap();
        assert_eq!(trace(&book, "main", &Limits::default()).unwrap().result, Some(Num::Int(7)));
        let book = crate::parse("@main = r & #-7 ~ $div(#2 r)").unwrap();
        assert_eq!(trace(&book, "main", &Limits::default()).unwrap().result, Some(Num::Int(-4)));
    }

    #[test]
    fn graph_of_a_definition() {
        let book = crate::parse("@id = (x x)\n@main = r & @id ~ (#5 r)").unwrap();
        let g = Graph::of_def(&book.defs[1]);
        // root, ref, con, num
        assert_eq!(g.live(), 4);
        assert_eq!(g.redexes().len(), 1, "@id ~ (#5 r) is active");
        let t = trace(&book, "main", &Limits { frame_nodes: 100, frames: 10, ..Limits::default() }).unwrap();
        assert_eq!(t.result, Some(Num::Int(5)));
        assert_eq!(t.frames.len(), t.steps.len() + 1, "a frame before each round and at the end");
    }

    #[test]
    fn a_call_on_numbers() {
        let mut book = crate::parse(include_str!("../../../tests/nets/fib.mnet")).unwrap();
        book.defs.retain(|d| d.name != "main");
        book.defs.push(call("fib", &[Num::Int(10)]));
        assert_eq!(trace(&book, "main", &Limits::default()).unwrap().result, Some(Num::Int(55)));
    }
}
