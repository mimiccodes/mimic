// brainf: an interpreter for Brainf**k running a program of three nested
// counting loops (about 15.8 million instructions). Prints the instructions
// run and a checksum of the tape.
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    char prog[512];
    int n = 0;
    for (int i = 0; i < 120; i++) prog[n++] = '+';
    n += sprintf(prog + n, "[>");
    for (int i = 0; i < 120; i++) prog[n++] = '+';
    n += sprintf(prog + n, "[>");
    for (int i = 0; i < 121; i++) prog[n++] = '+';
    n += sprintf(prog + n, "[>+>+<<-]<-]<-]");
    int jump[512], stack[64], sp = 0;
    for (int i = 0; i < n; i++) {
        if (prog[i] == '[') stack[sp++] = i;
        else if (prog[i] == ']') { int j = stack[--sp]; jump[i] = j; jump[j] = i; }
    }
    uint8_t tape[16] = {0};
    int64_t steps = 0;
    for (int pc = 0, ptr = 0; pc < n; pc++) {
        steps++;
        switch (prog[pc]) {
        case '+': tape[ptr]++; break;
        case '-': tape[ptr]--; break;
        case '>': ptr++; break;
        case '<': ptr--; break;
        case '[': if (!tape[ptr]) pc = jump[pc]; break;
        case ']': if (tape[ptr]) pc = jump[pc]; break;
        }
    }
    int64_t check = 0;
    for (int i = 0; i < 16; i++) check += (i + 1) * tape[i];
    printf("%lld %lld\n", (long long)steps, (long long)check);
    return 0;
}
