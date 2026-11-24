// Union-find with path compression over n elements and 2,000,000 generated
// unions; print the number of sets and the size of the largest one.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

static int64_t *parent;

static int64_t find(int64_t i) {
    if (parent[i] == i) return i;
    int64_t root = find(parent[i]);
    parent[i] = root;
    return root;
}

int main(void) {
    const int64_t n = 1000003, unions = 2000000;
    int64_t x = 42;
    parent = malloc(sizeof(int64_t) * (size_t)n);
    for (int64_t i = 0; i < n; i++) parent[i] = i;
    for (int64_t k = 0; k < unions; k++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        int64_t a = x % n;
        x = (x * 1103515245 + 12345) % 2147483648;
        int64_t b = x % n;
        int64_t ra = find(a), rb = find(b);
        if (ra != rb) parent[ra] = rb;
    }
    int64_t *size = calloc((size_t)n, sizeof(int64_t));
    for (int64_t i = 0; i < n; i++) size[find(i)]++;
    int64_t sets = 0, largest = 0;
    for (int64_t i = 0; i < n; i++) {
        if (size[i] > 0) sets++;
        if (size[i] > largest) largest = size[i];
    }
    printf("%lld\n%lld\n", (long long)sets, (long long)largest);
    free(parent);
    free(size);
    return 0;
}
