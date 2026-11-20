// bigfact: 20,000! as limbs of 9 decimal digits, multiplied by 2 … 20,000
// one after another. Prints the number of decimal digits, their sum, and
// the number of trailing zeros.

const BASE: i64 = 1_000_000_000;

fn main() {
    let mut l: Vec<i64> = vec![1];
    for k in 2..=20_000i64 {
        let mut carry = 0;
        for x in l.iter_mut() {
            let v = *x * k + carry;
            *x = v % BASE;
            carry = v / BASE;
        }
        while carry > 0 {
            l.push(carry % BASE);
            carry /= BASE;
        }
    }
    let top = *l.last().unwrap();
    let digits = 9 * (l.len() - 1) + top.to_string().len();
    let sum: i64 = l.iter().map(|&x| x.to_string().bytes().map(|b| (b - b'0') as i64).sum::<i64>()).sum();
    let i = l.iter().position(|&x| x != 0).unwrap();
    let mut zeros = 9 * i;
    let mut x = l[i];
    while x % 10 == 0 {
        zeros += 1;
        x /= 10;
    }
    println!("{} {} {}", digits, sum, zeros);
}
