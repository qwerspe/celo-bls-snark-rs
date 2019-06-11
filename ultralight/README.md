# BLS-ZEXE

Implements BLS signatures as described in [BDN18].

## Using the code
### Quick start

The `simple_signature` program shows how to generate keys, sign and aggregate signatures.

To run it with debug logging enabled, execute:

`RUST_LOG=debug cargo +nightly run --example simple_signature -- -m hello`

### Building

To build the project, you must use the nightly version. This is because [ZEXE](https://github.com/scipr-lab/zexe) uses the `const_fn` feature.

`cargo +nightly build` 

or 

`cargo +nightly build --release`

### Running tests

Most of the modules have tests.
 
 You should run tests in release mode, as some of the cryptographic operations are slow in debug mode.

`cargo +nightly test`

## Construction

We work over the $E_{CP}$ curve from [BCGMMW18].

Secret keys are elements of the scalar field *Fr*.

We would like to minimize the public key size, since we expect many of them to be communicated. Therefore, public keys are in *G1* and signatures are in *G2*.

To hash a message to *G2*, we currently use the try-and-increment method coupled with a composite hash. The composite hash is composed of a Pedersen hash over $E_{Ed/CP}$ from [BCGMMW18] and SHA256. First, the Pedersen hash is applied to the message, and then the try-and-increment methods attempts incrementing counters over the hashed message using SHA256.

We implement fast cofactor multiplication, as the *G2* cofactor is large.

## References

[BDN18] Boneh, D., Drijvers, M., & Neven, G. (2018, December). [Compact multi-signatures for smaller blockchains](https://eprint.iacr.org/2018/483.pdf). In International Conference on the Theory and Application of Cryptology and Information Security (pp. 435-464). Springer, Cham.

[BLS01] Boneh, D., Lynn, B., & Shacham, H. (2001, December). [Short signatures from the Weil pairing](https://link.springer.com/content/pdf/10.1007/3-540-45682-1_30.pdf). In International Conference on the Theory and Application of Cryptology and Information Security (pp. 514-532). Springer, Berlin, Heidelberg.

[BCGMMW18] Bowe, S., Chiesa, A., Green, M., Miers, I., Mishra, P., & Wu, H. (2018). [Zexe: Enabling decentralized private computation](https://eprint.iacr.org/2018/962.pdf). IACR ePrint, 962.

[pairings] Costello, C. . [Pairings for beginners](http://www.craigcostello.com.au/pairings/PairingsForBeginners.pdf).

[BP17] Budroni, A., & Pintore, F. (2017). [Efficient hash maps to G2 on BLS curves](https://eprint.iacr.org/2017/419.pdf). Cryptology ePrint Archive, Report 2017/419.