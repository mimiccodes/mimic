# The reference for semver (pure Python): 1,000,000 version strings
# MAJOR.MINOR.PATCH with an optional pre-release (-alpha, -alpha.N, -beta.N,
# -rc.N), from splitmix64, parsed and sorted by Semantic Versioning 2.0.0
# precedence: numbers compare as numbers, a version with a pre-release
# comes before the same version without, and alpha < beta < rc, a bare
# identifier before the same one with a number. Prints the first, the
# median and the last version, and how many have a pre-release. The sort
# here compares parsed tuples; the benchmark may encode them as one number.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def version(i):
    s = f"{mix(i) % 20}.{mix(i + 10**7) % 30}.{mix(i + 2 * 10**7) % 200}"
    r, k = mix(i + 3 * 10**7) % 10, mix(i + 4 * 10**7) % 20
    return s + ["", "", "", "", "", "", f"-alpha.{k}", f"-beta.{k}", f"-rc.{k}", "-alpha"][r]


def key(v):
    core, _, pre = v.partition("-")
    nums = tuple(int(x) for x in core.split("."))
    if not pre:
        return nums + (1,)
    ids = pre.split(".")
    return nums + (0, ids[0], tuple(int(x) for x in ids[1:]))


text = "\n".join(version(i) for i in range(1000000))
vs = sorted(text.split("\n"), key=key)
print(vs[0], vs[len(vs) // 2], vs[-1], sum(1 for v in vs if "-" in v))
