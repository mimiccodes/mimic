# The reference for catalan (Python, by the closed form): the Catalan
# numbers C(0) … C(20,000) modulo 1,000,000,007. (The benchmark computes
# them by the convolution C(n + 1) = sum of C(i) C(n - i); the closed form
# C(n) = (2n)! / (n! (n + 1)!) checks it.) Prints C(20,000) and the sum of
# all of them modulo the prime.
P = 1000000007
N = 20000
fact = [1] * (2 * N + 2)
for i in range(1, 2 * N + 2):
    fact[i] = fact[i - 1] * i % P
inv = lambda x: pow(x, P - 2, P)
cs = [fact[2 * n] * inv(fact[n]) % P * inv(fact[n + 1]) % P for n in range(N + 1)]
print(cs[N], sum(cs) % P)
