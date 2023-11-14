//! Threshold signing for `decaf377-rdsa` signatures via FROST.
//!
//! This implementation only supports producing `SpendAuth` signatures, which
//! use the conventional `decaf377` basepoint.

use anyhow::anyhow;
use frost_core::frost;
use penumbra_proto::crypto::decaf377_frost::v1alpha1 as pb;
use std::collections::HashMap;

/// A FROST-related error.
pub type Error = frost_core::Error<traits::Decaf377Rdsa>;

use rand_core::{self, CryptoRng, RngCore};

mod hash;
pub mod keys;
mod traits;

use decaf377_rdsa::{Signature, SpendAuth};

// TODO: properly factor this code into leaf modules

// Below code copied from frost-ed25519 ("MIT or Apache-2.0")

type E = traits::Decaf377Rdsa;

/// A FROST participant identifier.
pub type Identifier = frost::Identifier<E>;

/// Signing round 1 functionality and types.
pub mod round1 {
    use crate::keys::SigningShare;

    use super::*;

    /// The nonces used for a single FROST signing ceremony.
    ///
    /// Note that [`SigningNonces`] must be used *only once* for a signing
    /// operation; re-using nonces will result in leakage of a signer's long-lived
    /// signing key.
    pub type SigningNonces = frost::round1::SigningNonces<E>;

    /// Published by each participant in the first round of the signing protocol.
    ///
    /// This step can be batched if desired by the implementation. Each
    /// SigningCommitment can be used for exactly *one* signature.
    #[derive(Debug, Clone)]
    pub struct SigningCommitments(frost::round1::SigningCommitments<E>);

    impl From<SigningCommitments> for pb::SigningCommitments {
        fn from(value: SigningCommitments) -> Self {
            Self {
                hiding: Some(pb::NonceCommitment {
                    element: value.0.hiding().serialize(),
                }),
                binding: Some(pb::NonceCommitment {
                    element: value.0.binding().serialize(),
                }),
            }
        }
    }

    impl TryFrom<pb::SigningCommitments> for SigningCommitments {
        type Error = anyhow::Error;

        fn try_from(value: pb::SigningCommitments) -> Result<Self, Self::Error> {
            Ok(Self(frost::round1::SigningCommitments::new(
                frost::round1::NonceCommitment::deserialize(
                    value
                        .hiding
                        .ok_or(anyhow!("SigningCommitments missing hiding"))?
                        .element,
                )?,
                frost::round1::NonceCommitment::deserialize(
                    value
                        .binding
                        .ok_or(anyhow!("SigningCommitments missing binding"))?
                        .element,
                )?,
            )))
        }
    }

    /// Performed once by each participant selected for the signing operation.
    ///
    /// Generates the signing nonces and commitments to be used in the signing
    /// operation.
    pub fn commit<RNG>(secret: &SigningShare, rng: &mut RNG) -> (SigningNonces, SigningCommitments)
    where
        RNG: CryptoRng + RngCore,
    {
        let (a, b) = frost::round1::commit::<E, RNG>(secret, rng);
        (a, SigningCommitments(b))
    }
}

/// Generated by the coordinator of the signing operation and distributed to
/// each signing party.
pub type SigningPackage = frost::SigningPackage<E>;

/// Signing Round 2 functionality and types.
pub mod round2 {
    use frost_rerandomized::Randomizer;

    use super::*;

    /// A FROST participant's signature share, which the Coordinator will
    /// aggregate with all other signer's shares into the joint signature.
    #[derive(Debug, Clone)]
    pub struct SignatureShare(pub(crate) frost::round2::SignatureShare<E>);

    impl From<SignatureShare> for pb::SignatureShare {
        fn from(value: SignatureShare) -> Self {
            pb::SignatureShare {
                scalar: value.0.serialize(),
            }
        }
    }

    impl TryFrom<pb::SignatureShare> for SignatureShare {
        type Error = anyhow::Error;

        fn try_from(value: pb::SignatureShare) -> Result<Self, Self::Error> {
            Ok(Self(frost::round2::SignatureShare::deserialize(
                value.scalar,
            )?))
        }
    }

    /// Performed once by each participant selected for the signing operation.
    ///
    /// Receives the message to be signed and a set of signing commitments and a set
    /// of randomizing commitments to be used in that signing operation, including
    /// that for this participant.
    ///
    /// Assumes the participant has already determined which nonce corresponds with
    /// the commitment that was assigned by the coordinator in the SigningPackage.
    pub fn sign(
        signing_package: &SigningPackage,
        signer_nonces: &round1::SigningNonces,
        key_package: &keys::KeyPackage,
    ) -> Result<SignatureShare, Error> {
        frost::round2::sign(signing_package, signer_nonces, key_package).map(SignatureShare)
    }

    /// Like [`sign`], but for producing signatures with a randomized verification key.
    pub fn sign_randomized(
        signing_package: &SigningPackage,
        signer_nonces: &round1::SigningNonces,
        key_package: &keys::KeyPackage,
        randomizer: decaf377::Fr,
    ) -> Result<SignatureShare, Error> {
        frost_rerandomized::sign(
            signing_package,
            signer_nonces,
            key_package,
            Randomizer::from_scalar(randomizer),
        )
        .map(SignatureShare)
    }
}

/// Verifies each FROST participant's signature share, and if all are valid,
/// aggregates the shares into a signature to publish.
///
/// The resulting signature is an ordinary Schnorr signature with normal
/// verification.
///
/// This operation is performed by a coordinator that can communicate with all
/// the signing participants before publishing the final signature. The
/// coordinator can be one of the participants or a semi-trusted third party
/// (who is trusted to not perform denial of service attacks, but does not learn
/// any secret information).
///
/// Note that because the coordinator is trusted to report misbehaving parties
/// in order to avoid publishing an invalid signature, if the coordinator
/// themselves is a signer and misbehaves, they can avoid that step. However, at
/// worst, this results in a denial of service attack due to publishing an
/// invalid signature.
pub fn aggregate(
    signing_package: &SigningPackage,
    signature_shares: &HashMap<Identifier, round2::SignatureShare>,
    pubkeys: &keys::PublicKeyPackage,
) -> Result<Signature<SpendAuth>, Error> {
    let signature_shares = signature_shares
        .iter()
        .map(|(a, b)| (*a, b.0.clone()))
        .collect();
    let frost_sig = frost::aggregate(signing_package, &signature_shares, pubkeys)?;
    Ok(TryInto::<[u8; 64]>::try_into(frost_sig.serialize())
        .expect("serialization is valid")
        .into())
}

/// Like [`aggregate`], but for generating signatures with a randomized
/// verification key.
pub fn aggregate_randomized(
    signing_package: &SigningPackage,
    signature_shares: &HashMap<Identifier, round2::SignatureShare>,
    pubkeys: &keys::PublicKeyPackage,
    randomizer: decaf377::Fr,
) -> Result<Signature<SpendAuth>, Error> {
    let signature_shares = signature_shares
        .iter()
        .map(|(a, b)| (*a, b.0.clone()))
        .collect();
    let frost_sig = frost_rerandomized::aggregate(
        signing_package,
        &signature_shares,
        pubkeys,
        &frost_rerandomized::RandomizedParams::from_randomizer(
            pubkeys.group_public(),
            frost_rerandomized::Randomizer::from_scalar(randomizer),
        ),
    )?;
    Ok(TryInto::<[u8; 64]>::try_into(frost_sig.serialize())
        .expect("serialization is valid")
        .into())
}
