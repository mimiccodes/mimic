// 500 bank accounts; applies generated deposits, withdrawals (refused when
// the balance is short) and transfers, then prints the total money, the
// richest account and the number of refusals.
#include <stdint.h>
#include <stdio.h>

#define ACCOUNTS 500
#define OPS 2000000

typedef struct {
    int64_t id;
    int64_t balance;
} Account;

static int64_t x = 42;

static int64_t next(void) {
    x = (x * 1103515245 + 12345) % 2147483648;
    return x;
}

int main(void) {
    Account accounts[ACCOUNTS];
    for (int i = 0; i < ACCOUNTS; i++) {
        accounts[i].id = i;
        accounts[i].balance = 1000;
    }
    int64_t refused = 0;
    for (int op = 0; op < OPS; op++) {
        int64_t kind = next() % 3;
        int64_t a = next() % ACCOUNTS;
        int64_t amount = next() % 1000 + 1;
        if (kind == 0) {
            accounts[a].balance += amount;
        } else if (kind == 1) {
            if (accounts[a].balance >= amount) accounts[a].balance -= amount;
            else refused++;
        } else {
            int64_t b = next() % ACCOUNTS;
            if (accounts[a].balance >= amount) {
                accounts[a].balance -= amount;
                accounts[b].balance += amount;
            } else {
                refused++;
            }
        }
    }
    int64_t total = 0;
    int richest = 0;
    for (int i = 0; i < ACCOUNTS; i++) {
        total += accounts[i].balance;
        if (accounts[i].balance > accounts[richest].balance) richest = i;
    }
    printf("total %lld\n", (long long)total);
    printf("richest %lld with %lld\n", (long long)accounts[richest].id,
           (long long)accounts[richest].balance);
    printf("refused %lld\n", (long long)refused);
    return 0;
}
