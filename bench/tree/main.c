#include <stdio.h>
#include <stdlib.h>

/* allocation: build complete binary trees and count their nodes
 * (64 trees of depth 16, then one of depth 22) */
typedef struct node { struct node *l, *r; } node;

static node *make(int d) {
    node *n = malloc(sizeof *n);
    if (d == 0) { n->l = n->r = NULL; return n; }
    n->l = make(d - 1);
    n->r = make(d - 1);
    return n;
}

static long check(const node *t) { return t->l ? 1 + check(t->l) + check(t->r) : 1; }

static void drop(node *t) {
    if (t->l) { drop(t->l); drop(t->r); }
    free(t);
}

int main(void) {
    long s = 0;
    for (int i = 0; i < 64; i++) {
        node *t = make(16);
        s += check(t);
        drop(t);
    }
    printf("%ld\n", s);
    node *t = make(22);
    printf("%ld\n", check(t));
    drop(t);
    return 0;
}
