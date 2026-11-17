// vm: a stack-based bytecode virtual machine executing a loop of 400,000
// passes, acc = (acc × 31 + i × i) mod 1,000,003 (about 7.6 million
// instructions). Prints the instructions run and the result.

#[derive(Clone, Copy)]
enum Op {
    Push(i64),
    Load(usize),
    Store(usize),
    Add,
    Mul,
    Mod,
    Lt,
    Jz(usize),
    Jmp(usize),
    Halt,
}

fn main() {
    use Op::*;
    let prog = [
        Push(0), Store(0), Push(0), Store(1),
        Load(0), Push(400000), Lt, Jz(23),
        Load(1), Push(31), Mul, Load(0), Load(0), Mul, Add,
        Push(1000003), Mod, Store(1),
        Load(0), Push(1), Add, Store(0), Jmp(4),
        Halt,
    ];
    let mut stack = Vec::with_capacity(16);
    let mut regs = [0i64; 4];
    let (mut pc, mut steps) = (0usize, 0i64);
    loop {
        steps += 1;
        match prog[pc] {
            Push(x) => { stack.push(x); pc += 1; }
            Load(r) => { stack.push(regs[r]); pc += 1; }
            Store(r) => { regs[r] = stack.pop().unwrap(); pc += 1; }
            Jz(t) => { pc = if stack.pop().unwrap() == 0 { t } else { pc + 1 }; }
            Jmp(t) => pc = t,
            Halt => break,
            op => {
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                stack.push(match op { Add => a + b, Mul => a * b, Mod => a % b, _ => (a < b) as i64 });
                pc += 1;
            }
        }
    }
    println!("{steps} {}", regs[1]);
}
