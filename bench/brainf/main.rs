// brainf: an interpreter for Brainf**k running a program of three nested
// counting loops (about 15.8 million instructions). Prints the instructions
// run and a checksum of the tape.

fn main() {
    let prog: Vec<u8> = format!("{}[>{}[>{}[>+>+<<-]<-]<-]", "+".repeat(120), "+".repeat(120), "+".repeat(121)).into_bytes();
    let mut jump = vec![0usize; prog.len()];
    let mut stack = Vec::new();
    for (i, &c) in prog.iter().enumerate() {
        if c == b'[' {
            stack.push(i);
        } else if c == b']' {
            let j = stack.pop().unwrap();
            jump[i] = j;
            jump[j] = i;
        }
    }
    let mut tape = [0u8; 16];
    let (mut pc, mut ptr, mut steps) = (0usize, 0usize, 0i64);
    while pc < prog.len() {
        steps += 1;
        match prog[pc] {
            b'+' => tape[ptr] = tape[ptr].wrapping_add(1),
            b'-' => tape[ptr] = tape[ptr].wrapping_sub(1),
            b'>' => ptr += 1,
            b'<' => ptr -= 1,
            b'[' if tape[ptr] == 0 => pc = jump[pc],
            b']' if tape[ptr] != 0 => pc = jump[pc],
            _ => {}
        }
        pc += 1;
    }
    let check: i64 = tape.iter().enumerate().map(|(i, &v)| (i as i64 + 1) * v as i64).sum();
    println!("{steps} {check}");
}
