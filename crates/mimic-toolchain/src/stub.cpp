// Built when the embedded toolchain's dependencies are missing
// (run tools/deps.sh): the entry points report that instead.
#include <cstdio>
extern "C" int mimic_lld_main(int, const char **) {
    std::fprintf(stderr, "error: this mimic was built without the embedded linker (run tools/deps.sh and rebuild)\n");
    return 1;
}
extern "C" int mimic_clang_main(int, char **, const char *) {
    std::fprintf(stderr, "error: this mimic was built without the embedded C compiler (run tools/deps.sh and rebuild)\n");
    return 1;
}
extern "C" int mimic_write_archive(const char *, int, int, const char **, char *err, unsigned long errlen) {
    const char *m = "this mimic was built without the embedded archiver";
    unsigned long i = 0;
    for (; m[i] && i + 1 < errlen; i++) err[i] = m[i];
    err[i] = 0;
    return 1;
}
extern "C" int mimic_list_cpus(const char *, int, void (*)(const char *, const char *, void *), void *) { return 1; }
extern "C" void mimic_llvm_names(int, void (*)(const char *, const char *, void *), void *) {}
extern "C" void mimic_llvm_triple(const char *in, char *out, unsigned long outlen) {
    unsigned long i = 0;
    for (; in[i] && i + 1 < outlen; i++) out[i] = in[i];
    out[i] = 0;
}
