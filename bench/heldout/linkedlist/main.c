// Build a singly linked list of generated values (each new value goes on
// the front), reverse it, and sum every third value (positions 0, 3, 6, ...).
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct Cell {
    int64_t value;
    struct Cell *next;
} Cell;

int main(void) {
    const int64_t n = 2000000;
    int64_t x = 42;
    Cell *list = NULL;
    for (int64_t i = 0; i < n; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        Cell *c = malloc(sizeof(Cell));
        c->value = x % 1001;
        c->next = list;
        list = c;
    }

    Cell *reversed = NULL;
    while (list != NULL) {
        Cell *rest = list->next;
        list->next = reversed;
        reversed = list;
        list = rest;
    }

    int64_t sum = 0, i = 0, length = 0;
    for (Cell *c = reversed; c != NULL; c = c->next, i++) {
        if (i % 3 == 0) sum += c->value;
        length++;
    }
    printf("%lld\n%lld\n", (long long)length, (long long)sum);

    while (reversed != NULL) {
        Cell *rest = reversed->next;
        free(reversed);
        reversed = rest;
    }
    return 0;
}
