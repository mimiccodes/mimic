# The reference for brainf (Python): the same program run by a plain
# interpreter. The program: A × B × C passes of an inner loop that moves a
# cell into two others (cells wrap at 256); prints the instructions run and
# a checksum of the tape.
A, B, C = 120, 120, 121
prog = "+" * A + "[>" + "+" * B + "[>" + "+" * C + "[>+>+<<-]<-]<-]"
jump = {}
stack = []
for i, ch in enumerate(prog):
    if ch == "[":
        stack.append(i)
    elif ch == "]":
        j = stack.pop()
        jump[i], jump[j] = j, i
tape = [0] * 16
pc = ptr = steps = 0
while pc < len(prog):
    ch = prog[pc]
    steps += 1
    if ch == "+":
        tape[ptr] = (tape[ptr] + 1) % 256
    elif ch == "-":
        tape[ptr] = (tape[ptr] - 1) % 256
    elif ch == ">":
        ptr += 1
    elif ch == "<":
        ptr -= 1
    elif ch == "[" and tape[ptr] == 0:
        pc = jump[pc]
    elif ch == "]" and tape[ptr] != 0:
        pc = jump[pc]
    pc += 1
print(steps, sum((i + 1) * v for i, v in enumerate(tape)))
