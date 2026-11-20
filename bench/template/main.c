// template: a template with six {{placeholders}} rendered 300,000 times
// with values from splitmix64, scanning for "{{" and "}}" on each render.
// Prints the total length of the rendered texts and the sum of their
// character codes.
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static const char *ITEMS[5] = {"lamp", "kettle", "chair", "notebook", "umbrella"};
static const char *DAYS[7] = {"Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"};

int main(void) {
    const char *t = "Dear {{name}}, your order {{id}} of {{qty}} x {{item}} ships on {{day}}. Total: {{total}}.";
    int64_t length = 0, codes = 0;
    char out[256], val[64];
    for (int64_t r = 0; r < 300000; r++) {
        int64_t qty = 1 + mix(r) % 9, o = 0;
        for (const char *p = t; *p;) {
            if (p[0] == '{' && p[1] == '{') {
                const char *k = p + 2, *e = strstr(k, "}}");
                size_t kl = (size_t)(e - k);
                if (kl == 4 && !strncmp(k, "name", 4)) sprintf(val, "user%lld", (long long)(r % 1000));
                else if (kl == 2 && !strncmp(k, "id", 2)) sprintf(val, "%lld", (long long)r);
                else if (kl == 3 && !strncmp(k, "qty", 3)) sprintf(val, "%lld", (long long)qty);
                else if (kl == 4 && !strncmp(k, "item", 4)) strcpy(val, ITEMS[mix(r + 1) % 5]);
                else if (kl == 3 && !strncmp(k, "day", 3)) strcpy(val, DAYS[mix(r + 2) % 7]);
                else sprintf(val, "%lld", (long long)(qty * (100 + mix(r + 3) % 900)));
                size_t vl = strlen(val);
                memcpy(out + o, val, vl);
                o += (int64_t)vl;
                p = e + 2;
            } else out[o++] = *p++;
        }
        length += o;
        for (int64_t i = 0; i < o; i++) codes += (unsigned char)out[i];
    }
    printf("%lld %lld\n", (long long)length, (long long)codes);
    return 0;
}
