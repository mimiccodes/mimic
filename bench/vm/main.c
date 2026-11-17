// vm: a stack-based bytecode virtual machine executing a loop of 400,000
// passes, acc = (acc × 31 + i × i) mod 1,000,003 (about 7.6 million
// instructions). Prints the instructions run and the result.
#include <stdint.h>
#include <stdio.h>

enum { PUSH, LOAD, STORE, ADD, MUL, MOD, LT, JZ, JMP, HALT };

int main(void) {
    static const int64_t prog[][2] = {
        {PUSH, 0}, {STORE, 0}, {PUSH, 0}, {STORE, 1},
        {LOAD, 0}, {PUSH, 400000}, {LT, 0}, {JZ, 23},
        {LOAD, 1}, {PUSH, 31}, {MUL, 0}, {LOAD, 0}, {LOAD, 0}, {MUL, 0}, {ADD, 0},
        {PUSH, 1000003}, {MOD, 0}, {STORE, 1},
        {LOAD, 0}, {PUSH, 1}, {ADD, 0}, {STORE, 0}, {JMP, 4},
        {HALT, 0}};
    int64_t stack[16], regs[4] = {0}, steps = 0;
    int sp = 0;
    for (int64_t pc = 0; pc >= 0;) {
        int64_t op = prog[pc][0], x = prog[pc][1];
        steps++;
        switch (op) {
        case PUSH: stack[sp++] = x; pc++; break;
        case LOAD: stack[sp++] = regs[x]; pc++; break;
        case STORE: regs[x] = stack[--sp]; pc++; break;
        case JZ: pc = stack[--sp] == 0 ? x : pc + 1; break;
        case JMP: pc = x; break;
        case HALT: pc = -1; break;
        default: {
            int64_t a = stack[sp - 2], b = stack[sp - 1];
            stack[sp - 2] = op == ADD ? a + b : op == MUL ? a * b : op == MOD ? a % b : a < b;
            sp--;
            pc++;
        }
        }
    }
    printf("%lld %lld\n", (long long)steps, (long long)regs[1]);
    return 0;
}
