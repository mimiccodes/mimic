/* ABI conformance: shapes whose passing rules differ between platforms. */
struct tiny { char c; };                                    /* 1 byte: one register */
struct pair_f { float a, b; };                              /* HFA of floats (arm64 v-regs) */
struct quad_d { double a, b, c, d; };                       /* HFA of 4 doubles: largest in-register HFA */
struct mixed { int i; double d; };                          /* int + float classes (x86-64 SysV) */
struct big { long long a, b, c, d, e, f, g, h; };                    /* 64 bytes: passed in memory */
struct odd { char a; short b; char c; int d; };             /* padding */
struct nest { struct pair_f p; struct tiny t; double x; };  /* nesting */

struct tiny tiny_echo(struct tiny t);
struct pair_f pair_swap(struct pair_f p);
double quad_sum(struct quad_d q);
struct quad_d quad_scale(struct quad_d q, double k);
struct mixed mixed_bump(struct mixed m);
long long big_sum(struct big b);
struct big big_make(long long base);
struct odd odd_echo(struct odd o);
struct nest nest_make(float a, float b, char c, double x);
/* 10 struct arguments: exhausts argument registers, spills to the stack */
double many(struct pair_f a, struct pair_f b, struct pair_f c, struct pair_f d, struct pair_f e,
            struct mixed f, struct mixed g, struct mixed h, double i, long long j);
