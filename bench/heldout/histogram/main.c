// Buckets 20,000,000 generated values (0 to 99,999) into 100 bins of
// width 1,000; prints the 3 fullest bins.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define VALUES 20000000
#define BINS 100

static int64_t counts[BINS];

static int fuller(const void *a, const void *b) {
    int i = *(const int *)a, j = *(const int *)b;
    if (counts[i] != counts[j]) return counts[i] > counts[j] ? -1 : 1;
    return i - j;
}

int main(void) {
    int64_t x = 42;
    for (int n = 0; n < VALUES; n++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        counts[(x % 100000) / 1000]++;
    }
    int order[BINS];
    for (int i = 0; i < BINS; i++) order[i] = i;
    qsort(order, BINS, sizeof order[0], fuller);
    for (int i = 0; i < 3; i++) printf("bin %d: %lld\n", order[i], (long long)counts[order[i]]);
    return 0;
}
