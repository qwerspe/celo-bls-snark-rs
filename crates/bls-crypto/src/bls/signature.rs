use crate::curve::hash::HashToG1;

use algebra::{
    bls12_377::{
        g1::Parameters as Bls12_377G1Parameters, Bls12_377, Fq, Fq12, G1Affine, G1Projective,
        G2Affine, Parameters as Bls12_377Parameters,
    },
    bytes::{FromBytes, ToBytes},
    curves::SWModelParameters,
    AffineCurve, Field, One, PairingEngine, PrimeField, ProjectiveCurve, SquareRootField, Zero,
};
use std::borrow::Borrow;

use std::{
    io::{self, Read, Result as IoResult, Write},
    ops::Neg,
};

use super::{BLSError, PublicKey};

#[derive(Clone, Debug, PartialEq)]
pub struct Signature {
    sig: G1Projective,
}

impl Signature {
    pub fn from_sig(sig: G1Projective) -> Signature {
        Signature { sig }
    }

    pub fn get_sig(&self) -> G1Projective {
        self.sig
    }

    /// Sums the provided signatures to produce the aggregate signature.
    pub fn aggregate<S: Borrow<Signature>>(signatures: &[S]) -> Signature {
        let mut asig = G1Projective::zero();
        for i in signatures.iter() {
            asig = asig + &(*i).borrow().sig;
        }

        Signature { sig: asig }
    }

    /// Verifies the signature against a vector of pubkey & message tuples, for the provided
    /// messages domain.
    ///
    /// For each message, an optional extra_data field can be provided (empty otherwise).
    ///
    /// The provided hash_to_g1 implementation will be used to hash each message-extra_data pair
    /// to G1.
    ///
    /// The verification equation can be found in pg.11 from
    /// https://eprint.iacr.org/2018/483.pdf: "Batch verification"
    pub fn batch_verify<H: HashToG1, P: Borrow<PublicKey>>(
        &self,
        pubkeys: &[P],
        domain: &[u8],
        messages: &[(&[u8], &[u8])],
        hash_to_g1: &H,
    ) -> Result<(), BLSError> {
        let message_hashes = messages
            .iter()
            .map(|(message, extra_data)| {
                hash_to_g1
                    .hash::<Bls12_377Parameters>(domain, message, extra_data)
                    .map_err(|_| BLSError::HashToCurveFailed(message.to_vec(), extra_data.to_vec()))
            })
            .collect::<Result<Vec<G1Projective>, _>>()?;

        self.batch_verify_hashes(pubkeys, &message_hashes)
    }

    /// Verifies the signature against a vector of pubkey & message hash tuples
    /// This is a lower level method, if you prefer hashing to be done internally,
    /// consider using the `batch_verify` method.
    ///
    /// The verification equation can be found in pg.11 from
    /// https://eprint.iacr.org/2018/483.pdf: "Batch verification"
    pub fn batch_verify_hashes<P: Borrow<PublicKey>>(
        &self,
        pubkeys: &[P],
        message_hashes: &[G1Projective],
    ) -> Result<(), BLSError> {
        // `.into()` is needed to prepared the points
        let mut els = vec![(
            self.get_sig().into_affine().into(),
            G2Affine::prime_subgroup_generator().neg().into(),
        )];
        message_hashes
            .iter()
            .zip(pubkeys)
            .for_each(|(hash, pubkey)| {
                els.push((
                    hash.into_affine().into(),
                    pubkey.borrow().get_pk().into_affine().into(),
                ));
            });

        let pairing = Bls12_377::product_of_pairings(&els);
        if pairing == Fq12::one() {
            Ok(())
        } else {
            Err(BLSError::VerificationFailed)?
        }
    }
}

impl ToBytes for Signature {
    #[inline]
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        let affine = self.sig.into_affine();
        let mut x_bytes: Vec<u8> = vec![];
        let y_big = affine.y.into_repr();
        let half = Fq::modulus_minus_one_div_two();
        affine.x.write(&mut x_bytes)?;
        if y_big > half {
            let num_x_bytes = x_bytes.len();
            x_bytes[num_x_bytes - 1] |= 0x80;
        }
        writer.write(&x_bytes)?;
        Ok(())
    }
}

impl FromBytes for Signature {
    #[inline]
    fn read<R: Read>(mut reader: R) -> IoResult<Self> {
        let mut x_bytes_with_y: Vec<u8> = vec![];
        reader.read_to_end(&mut x_bytes_with_y)?;
        let x_bytes_with_y_len = x_bytes_with_y.len();
        let y_over_half = (x_bytes_with_y[x_bytes_with_y_len - 1] & 0x80) == 0x80;
        x_bytes_with_y[x_bytes_with_y_len - 1] &= 0xFF - 0x80;
        let x = Fq::read(x_bytes_with_y.as_slice())?;
        let x3b = <Bls12_377G1Parameters as SWModelParameters>::add_b(
            &((x.square() * &x) + &<Bls12_377G1Parameters as SWModelParameters>::mul_by_a(&x)),
        );
        let y = x3b.sqrt().ok_or(io::Error::new(
            io::ErrorKind::NotFound,
            "couldn't find square root for x",
        ))?;
        let negy = -y;
        let chosen_y = if (y <= negy) ^ y_over_half { y } else { negy };
        let sig = G1Affine::new(x, chosen_y, false);
        Ok(Signature::from_sig(sig.into_projective()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{keygen_batch, sign_batch, sum};
    use crate::{
        bls::{
            ffi::{Message, MessageFFI},
            PrivateKey, PublicKeyCache, SIG_DOMAIN,
        },
        curve::hash::try_and_increment::TryAndIncrement,
        hash::{composite::CompositeHasher, direct::DirectHasher, XOF},
    };
    use algebra::{bls12_377::G2Projective, UniformRand};
    use rand::{thread_rng, Rng};

    #[test]
    fn test_aggregated_sig() {
        let message = b"hello";
        let rng = &mut thread_rng();

        let composite_hasher = CompositeHasher::new().unwrap();
        let try_and_increment = TryAndIncrement::new(&composite_hasher);
        let sk1 = PrivateKey::generate(rng);
        let sk2 = PrivateKey::generate(rng);

        let sig1 = sk1.sign(&message[..], &[], &try_and_increment).unwrap();
        let sig2 = sk2.sign(&message[..], &[], &try_and_increment).unwrap();
        let sigs = &[sig1, sig2];

        let apk = PublicKeyCache::aggregate(&[sk1.to_public(), sk2.to_public()]);
        let asig = Signature::aggregate(sigs);
        apk.verify(&message[..], &[], &asig, &try_and_increment)
            .unwrap();
        apk.verify(&message[..], &[], &sigs[0], &try_and_increment)
            .unwrap_err();
        sk1.to_public()
            .verify(&message[..], &[], &asig, &try_and_increment)
            .unwrap_err();
        let message2 = b"goodbye";
        apk.verify(&message2[..], &[], &asig, &try_and_increment)
            .unwrap_err();

        let apk2 = PublicKeyCache::aggregate(&[sk1.to_public()]);
        apk2.verify(&message[..], &[], &asig, &try_and_increment)
            .unwrap_err();
        apk2.verify(&message[..], &[], &sigs[0], &try_and_increment)
            .unwrap();

        let apk3 = PublicKeyCache::aggregate(&[sk2.to_public(), sk1.to_public()]);
        apk3.verify(&message[..], &[], &asig, &try_and_increment)
            .unwrap();
        apk3.verify(&message[..], &[], &sigs[0], &try_and_increment)
            .unwrap_err();

        let apk4 = PublicKey::aggregate(&[sk1.to_public(), sk2.to_public()]);
        apk4.verify(&message[..], &[], &asig, &try_and_increment)
            .unwrap();
        apk4.verify(&message[..], &[], &sigs[0], &try_and_increment)
            .unwrap_err();
    }

    #[test]
    fn test_batch_verify() {
        let direct_hasher = DirectHasher::new().unwrap();
        let composite_hasher = CompositeHasher::new().unwrap();

        test_batch_verify_with_hasher(direct_hasher, false);
        test_batch_verify_with_hasher(composite_hasher, true);
    }

    fn test_batch_verify_with_hasher<X: XOF>(hasher: X, is_composite: bool) {
        let rng = &mut thread_rng();
        let try_and_increment = TryAndIncrement::new(&hasher);
        let num_epochs = 10;
        let num_validators = 7;

        // generate some msgs and extra data
        let mut msgs = Vec::new();
        for _ in 0..num_epochs {
            let message: Vec<u8> = (0..32).map(|_| rng.gen()).collect::<Vec<u8>>();
            let extra_data: Vec<u8> = (0..32).map(|_| rng.gen()).collect::<Vec<u8>>();
            msgs.push((message, extra_data));
        }
        let msgs = msgs
            .iter()
            .map(|(m, d)| (m.as_ref(), d.as_ref()))
            .collect::<Vec<_>>();

        // get each signed by a committee _on the same domain_ and get the agg sigs of the commitee
        let mut asig = G1Projective::zero();
        let mut pubkeys = Vec::new();
        let mut sigs = Vec::new();
        for i in 0..num_epochs {
            let mut epoch_pubkey = G2Projective::zero();
            let mut epoch_sig = G1Projective::zero();
            for _ in 0..num_validators {
                let sk = PrivateKey::generate(rng);
                let s = sk.sign(&msgs[i].0, &msgs[i].1, &try_and_increment).unwrap();

                epoch_sig += s.sig;
                epoch_pubkey += sk.to_public().as_ref();
            }

            pubkeys.push(PublicKey::from_pk(epoch_pubkey));
            sigs.push(Signature::from_sig(epoch_sig));

            asig += epoch_sig;
        }

        let asig = Signature::from_sig(asig);

        let res = asig.batch_verify(&pubkeys, SIG_DOMAIN, &msgs, &try_and_increment);

        assert!(res.is_ok());

        let mut messages = Vec::new();
        for i in 0..num_epochs {
            messages.push(Message {
                data: msgs[i].0,
                extra: msgs[i].1,
                public_key: &pubkeys[i],
                sig: &sigs[i],
            });
        }

        let msgs_ffi = messages
            .iter()
            .map(|m| MessageFFI::from(m))
            .collect::<Vec<_>>();

        let mut verified: bool = false;

        let success = crate::batch_verify_signature(
            &msgs_ffi[0] as *const MessageFFI,
            msgs_ffi.len(),
            is_composite,
            &mut verified as *mut bool,
        );
        assert!(success);
        assert!(verified);
    }

    #[test]
    fn batch_verify_hashes() {
        // generate 5 (aggregate sigs, message hash pairs)
        // verify them all in 1 call
        let batch_size = 5;
        let num_keys = 7;
        let rng = &mut rand::thread_rng();

        // generate some random messages
        let messages = (0..batch_size)
            .map(|_| G1Projective::rand(rng))
            .collect::<Vec<_>>();
        //
        // keygen for multiple rounds (7 keys per round)
        let (secret_keys, public_keys_batches) = keygen_batch::<Bls12_377>(batch_size, num_keys);

        // get the aggregate public key for each rounds
        let aggregate_pubkeys = public_keys_batches
            .iter()
            .map(|pks| sum(pks))
            .map(PublicKey::from_pk)
            .collect::<Vec<_>>();

        // the keys from each epoch sign the messages from the corresponding epoch
        let asigs = sign_batch::<Bls12_377>(&secret_keys, &messages);

        // get the complete aggregate signature
        let asig = sum(&asigs);
        let asig = Signature::from_sig(asig);

        let res = asig.batch_verify_hashes(&aggregate_pubkeys, &messages);

        assert!(res.is_ok());
    }
}