// Insert generated keys into a binary search tree (duplicates ignored),
// then count the keys in generated ranges [lo, lo + width) by walking the
// tree; print the number of keys, the total of the counts and the height.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct Node {
    int64_t key;
    struct Node *left, *right;
} Node;

static int64_t x = 42;

static int64_t next(void) {
    x = (x * 1103515245 + 12345) % 2147483648;
    return x;
}

static Node *insert(Node *root, int64_t key, int64_t *size) {
    Node **at = &root;
    while (*at != NULL) {
        if (key < (*at)->key) at = &(*at)->left;
        else if (key > (*at)->key) at = &(*at)->right;
        else return root;
    }
    Node *n = malloc(sizeof(Node));
    n->key = key;
    n->left = n->right = NULL;
    *at = n;
    (*size)++;
    return root;
}

static int64_t count_range(const Node *t, int64_t lo, int64_t hi) {
    if (t == NULL) return 0;
    if (t->key < lo) return count_range(t->right, lo, hi);
    if (t->key >= hi) return count_range(t->left, lo, hi);
    return 1 + count_range(t->left, lo, hi) + count_range(t->right, lo, hi);
}

static int64_t height(const Node *t) {
    if (t == NULL) return 0;
    int64_t l = height(t->left), r = height(t->right);
    return 1 + (l > r ? l : r);
}

static void release(Node *t) {
    if (t == NULL) return;
    release(t->left);
    release(t->right);
    free(t);
}

int main(void) {
    const int64_t keys = 300000, ranges = 10000;
    Node *root = NULL;
    int64_t size = 0;
    for (int64_t i = 0; i < keys; i++) root = insert(root, next() % 10000019, &size);
    int64_t total = 0;
    for (int64_t i = 0; i < ranges; i++) {
        int64_t lo = next() % 10000019;
        int64_t width = next() % 20011;
        total += count_range(root, lo, lo + width);
    }
    printf("%lld\n%lld\n%lld\n", (long long)size, (long long)total, (long long)height(root));
    release(root);
    return 0;
}
