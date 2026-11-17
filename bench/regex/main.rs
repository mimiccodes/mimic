// regex: a Thompson NFA engine. Four patterns are parsed, compiled into an
// automaton of at most 63 states and run over 50,000 generated strings
// each, the live states kept as the bits of one integer. Prints how many
// strings each pattern matches in full.

enum Re {
    Ch(u8, u8),
    Cat(Box<Re>, Box<Re>),
    Alt(Box<Re>, Box<Re>),
    Star(Box<Re>),
    Plus(Box<Re>),
    Opt(Box<Re>),
}

struct Parser<'a> {
    p: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn alt(&mut self) -> Re {
        let a = self.cat();
        if self.i < self.p.len() && self.p[self.i] == b'|' {
            self.i += 1;
            Re::Alt(Box::new(a), Box::new(self.alt()))
        } else {
            a
        }
    }
    fn cat(&mut self) -> Re {
        let a = self.rep();
        if self.i < self.p.len() && self.p[self.i] != b'|' && self.p[self.i] != b')' {
            Re::Cat(Box::new(a), Box::new(self.cat()))
        } else {
            a
        }
    }
    fn rep(&mut self) -> Re {
        let mut a = self.atom();
        while self.i < self.p.len() && matches!(self.p[self.i], b'*' | b'+' | b'?') {
            a = match self.p[self.i] {
                b'*' => Re::Star(Box::new(a)),
                b'+' => Re::Plus(Box::new(a)),
                _ => Re::Opt(Box::new(a)),
            };
            self.i += 1;
        }
        a
    }
    fn atom(&mut self) -> Re {
        let c = self.p[self.i];
        if c == b'(' {
            self.i += 1;
            let a = self.alt();
            self.i += 1;
            a
        } else if c == b'[' {
            let r = Re::Ch(self.p[self.i + 1], self.p[self.i + 3]);
            self.i += 5;
            r
        } else {
            self.i += 1;
            Re::Ch(c, c)
        }
    }
}

#[derive(Default)]
struct Nfa {
    kind: Vec<u8>,
    lo: Vec<u8>,
    hi: Vec<u8>,
    o1: Vec<usize>,
    o2: Vec<usize>,
}

impl Nfa {
    fn add(&mut self, k: u8, lo: u8, hi: u8, a: usize, b: usize) -> usize {
        self.kind.push(k);
        self.lo.push(lo);
        self.hi.push(hi);
        self.o1.push(a);
        self.o2.push(b);
        self.kind.len() - 1
    }
    fn comp(&mut self, r: &Re, next: usize) -> usize {
        match r {
            Re::Ch(lo, hi) => self.add(0, *lo, *hi, next, usize::MAX),
            Re::Cat(a, b) => {
                let sb = self.comp(b, next);
                self.comp(a, sb)
            }
            Re::Alt(a, b) => {
                let sa = self.comp(a, next);
                let sb = self.comp(b, next);
                self.add(1, 0, 0, sa, sb)
            }
            Re::Star(a) => {
                let s = self.add(1, 0, 0, usize::MAX, next);
                self.o1[s] = self.comp(a, s);
                s
            }
            Re::Plus(a) => {
                let s = self.add(1, 0, 0, usize::MAX, next);
                let body = self.comp(a, s);
                self.o1[s] = body;
                body
            }
            Re::Opt(a) => {
                let sa = self.comp(a, next);
                self.add(1, 0, 0, sa, next)
            }
        }
    }
    fn closure(&self, s: usize, seen: u64) -> u64 {
        if seen >> s & 1 == 1 {
            0
        } else if self.kind[s] == 1 {
            self.closure(self.o1[s], seen | 1 << s) | self.closure(self.o2[s], seen | 1 << s)
        } else {
            1 << s
        }
    }
}

fn mix(v: i64, m: i64) -> i64 {
    (v * 2654435761 >> 11) % m
}

fn count(pn: i64, pattern: &str, alpha: &[u8]) -> i64 {
    let ast = Parser { p: pattern.as_bytes(), i: 0 }.alt();
    let mut n = Nfa::default();
    let fin = n.add(2, 0, 0, usize::MAX, usize::MAX);
    let start = n.comp(&ast, fin);
    let cl: Vec<u64> = (0..n.kind.len()).map(|s| n.closure(s, 0)).collect();
    let mut c = 0;
    for j in 0..50000i64 {
        let len = 8 + mix(pn * 7919 + j, 33) as usize;
        let text: Vec<u8> = (0..len).map(|k| alpha[mix(pn * 1000003 + j * 131 + k as i64, alpha.len() as i64) as usize]).collect();
        let mut live = cl[start];
        let mut at = 0;
        while at < len && live != 0 {
            let ch = text[at];
            let mut next = 0;
            for s in 0..n.kind.len() {
                if live >> s & 1 == 1 && n.kind[s] == 0 && n.lo[s] <= ch && ch <= n.hi[s] {
                    next |= cl[n.o1[s]];
                }
            }
            live = next;
            at += 1;
        }
        c += (at == len && live >> fin & 1 == 1) as i64;
    }
    c
}

fn main() {
    let a = count(0, "(a|b)*abb", b"ab");
    let b = count(1, "(0|1(01*0)*1)*", b"01");
    let c = count(2, "[a-c]*b[a-c]?c+", b"abc");
    let d = count(3, "(x|y|z)*(xy|yz)+z?", b"xyz");
    println!("{a} {b} {c} {d}");
}
