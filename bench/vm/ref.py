# The reference for vm (Python, without a VM): the loop the bytecode runs,
# written out, and the instructions it takes (4 to set up, 19 a pass, 4
# for the last test, and the halt).
N = 400000
acc = 0
for i in range(N):
    acc = (acc * 31 + i * i) % 1000003
print(4 + 19 * N + 4 + 1, acc)
