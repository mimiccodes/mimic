// An inventory: a hash map from item name to (count, price). Applies
// generated orders (refused when the stock is short) and restocks, then
// prints the stock value and the five items with the lowest counts.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define ITEMS 1000
#define OPS 1000000
#define TABLE 2048

typedef struct {
    char name[16];
    int64_t count;
    int64_t price;
    int used;
} Entry;

static Entry table[TABLE];
static int order[ITEMS];  // table slots in insertion order

static uint64_t hash(const char *s) {
    uint64_t h = 1469598103934665603ULL;
    for (; *s; s++) {
        h ^= (unsigned char)*s;
        h *= 1099511628211ULL;
    }
    return h;
}

static int find_slot(const char *name) {
    uint64_t i = hash(name) & (TABLE - 1);
    while (table[i].used && strcmp(table[i].name, name) != 0) i = (i + 1) & (TABLE - 1);
    return (int)i;
}

static int64_t seed = 42;

static int64_t next(void) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
}

static int by_count(const void *a, const void *b) {
    const Entry *ea = &table[order[*(const int *)a]];
    const Entry *eb = &table[order[*(const int *)b]];
    if (ea->count != eb->count) return ea->count < eb->count ? -1 : 1;
    return *(const int *)a - *(const int *)b;
}

int main(void) {
    char name[16];
    for (int k = 0; k < ITEMS; k++) {
        snprintf(name, sizeof name, "item%d", k);
        int s = find_slot(name);
        strcpy(table[s].name, name);
        table[s].used = 1;
        table[s].count = next() % 100 + 1;
        table[s].price = next() % 500 + 1;
        order[k] = s;
    }
    int64_t refused = 0;
    for (int op = 0; op < OPS; op++) {
        int64_t kind = next() % 3;
        int64_t k = next() % ITEMS;
        int64_t qty = next() % 20 + 1;
        snprintf(name, sizeof name, "item%lld", (long long)k);
        Entry *e = &table[find_slot(name)];
        if (kind < 2) {
            if (e->count >= qty) e->count -= qty;
            else refused++;
        } else {
            e->count += 2 * qty;
        }
    }
    int64_t value = 0;
    for (int k = 0; k < ITEMS; k++) value += table[order[k]].count * table[order[k]].price;
    int idx[ITEMS];
    for (int k = 0; k < ITEMS; k++) idx[k] = k;
    qsort(idx, ITEMS, sizeof idx[0], by_count);
    printf("stock value %lld\n", (long long)value);
    printf("refused %lld\n", (long long)refused);
    for (int k = 0; k < 5; k++) {
        Entry *e = &table[order[idx[k]]];
        printf("%s %lld\n", e->name, (long long)e->count);
    }
    return 0;
}
