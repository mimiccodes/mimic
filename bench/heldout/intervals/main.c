// Sorts 1,000,000 generated intervals by start and merges the overlapping
// ones; prints how many merged intervals remain and their total length.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define COUNT 1000000

typedef struct {
    int64_t start, end;
} Interval;

static int by_start(const void *a, const void *b) {
    int64_t x = ((const Interval *)a)->start, y = ((const Interval *)b)->start;
    return (x > y) - (x < y);
}

int main(void) {
    Interval *iv = malloc(COUNT * sizeof *iv);
    int64_t x = 42;
    for (int i = 0; i < COUNT; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        iv[i].start = x % 1000000000;
        x = (x * 1103515245 + 12345) % 2147483648;
        iv[i].end = iv[i].start + x % 1000 + 1;
    }
    qsort(iv, COUNT, sizeof *iv, by_start);
    int64_t merged = 0, covered = 0;
    Interval cur = iv[0];
    for (int i = 1; i < COUNT; i++) {
        if (iv[i].start <= cur.end) {
            if (iv[i].end > cur.end) cur.end = iv[i].end;
        } else {
            merged++;
            covered += cur.end - cur.start;
            cur = iv[i];
        }
    }
    merged++;
    covered += cur.end - cur.start;
    printf("%lld\n", (long long)merged);
    printf("%lld\n", (long long)covered);
    free(iv);
    return 0;
}
