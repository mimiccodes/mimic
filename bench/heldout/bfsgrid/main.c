// Breadth-first search on a generated n x n grid with walls, from the top
// left corner; print the shortest path length to the bottom right corner
// (-1 if there is none) and how many cells can be reached.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdbool.h>

int main(void) {
    const int64_t n = 1500;
    const int64_t total = n * n;
    int64_t x = 42;
    bool *wall = malloc(sizeof(bool) * (size_t)total);
    for (int64_t i = 0; i < total; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        wall[i] = x % 7 < 2;
    }
    wall[0] = false;
    wall[total - 1] = false;

    int64_t *dist = malloc(sizeof(int64_t) * (size_t)total);
    int64_t *queue = malloc(sizeof(int64_t) * (size_t)total);
    for (int64_t i = 0; i < total; i++) dist[i] = -1;
    int64_t head = 0, tail = 0;
    dist[0] = 0;
    queue[tail++] = 0;
    const int64_t dr[4] = {-1, 1, 0, 0};
    const int64_t dc[4] = {0, 0, -1, 1};
    while (head < tail) {
        int64_t cell = queue[head++];
        int64_t r = cell / n, c = cell % n;
        for (int k = 0; k < 4; k++) {
            int64_t nr = r + dr[k], nc = c + dc[k];
            if (nr < 0 || nr >= n || nc < 0 || nc >= n) continue;
            int64_t i = nr * n + nc;
            if (wall[i] || dist[i] != -1) continue;
            dist[i] = dist[cell] + 1;
            queue[tail++] = i;
        }
    }
    printf("%lld\n%lld\n", (long long)dist[total - 1], (long long)tail);
    free(wall);
    free(dist);
    free(queue);
    return 0;
}
