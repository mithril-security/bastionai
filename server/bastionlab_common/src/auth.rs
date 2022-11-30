use crate::prelude::*;
use ring::{
    digest::{digest, SHA256},
    signature,
};
use std::{fs, path::Path};
use tonic::{metadata::MetadataMap, Status};
use x509_parser::prelude::Pem;

pub type PubKey = Vec<u8>;

#[derive(Debug, Default, Clone)]
pub struct KeyManagement {
    pub_keys: HashMap<String, PubKey>,
}

impl KeyManagement {
    fn read_from_file(path: &Path) -> Result<(String, PubKey)> {
        let file = &fs::read(path).with_context(|| anyhow!("Reading file: {path:?}"))?;
        let (_, Pem { contents, .. }) = x509_parser::pem::parse_x509_pem(&file[..])
            .map_err(|e| Status::aborted(e.to_string()))
            .with_context(|| anyhow!("Parsing PEM file: {path:?}"))?;

        let hash = hex::encode(digest(&SHA256, &contents[..]));
        Ok((hash, contents))
    }

    pub fn load_from_dir(path: &Path) -> Result<Self> {
        let mut pub_keys: HashMap<String, PubKey> = HashMap::new();

        ensure!(path.is_dir(), "keys path is not a folder");

        let paths =
            fs::read_dir(path).with_context(|| anyhow!("Listing files in directory: {path:?}"))?;

        for entry in paths {
            let entry = entry.with_context(|| anyhow!("Listing files in directory: {path:?}"))?;
            let file = entry.path();

            let (hash, raw) = KeyManagement::read_from_file(&file)
                .with_context(|| anyhow!("Reading file: {:?}", &file))?;
            pub_keys.insert(hash, raw);
        }

        Ok(KeyManagement { pub_keys })
    }

    pub fn verify_signature(
        &self,
        public_key_hash: &str,
        message: &[u8],
        header: &MetadataMap,
    ) -> Result<(), Status> {
        /*
            For authentication, first of we check if the provided public key exists in the list of public keys
            provided at start-up (owners, users).

            If it exists, we go ahead to then verify the signature received from the client by verifying the signature
            with the loaded public key created using `signature::UnparsedPublicKey::new`.
        */
        match header.get_bin(format!("signature-{}-bin", public_key_hash)) {
            Some(signature) => {
                let keys = &self.pub_keys;

                if let Some(raw_pub) = keys.get(&public_key_hash.to_string()) {
                    let public_key = spki::SubjectPublicKeyInfo::try_from(raw_pub.as_ref())
                        .map_err(|_| {
                            Status::invalid_argument(format!(
                                "Invalid SubjectPublicKeyInfo for pubkey {}",
                                public_key_hash
                            ))
                        })?;
                    let public_key = signature::UnparsedPublicKey::new(
                        &signature::ECDSA_P256_SHA256_ASN1,
                        public_key.subject_public_key,
                    );

                    let sign = signature.to_bytes().map_err(|_| {
                        Status::invalid_argument(format!(
                            "Could not decode signature for public key {}",
                            public_key_hash
                        ))
                    })?;

                    public_key.verify(message, &sign).map_err(|_| {
                        Status::permission_denied(format!(
                            "Invalid signature for public key {}",
                            public_key_hash
                        ))
                    })?;
                    return Ok(());
                }
                Err(Status::aborted(format!(
                    "{:?} not authenticated!",
                    public_key_hash
                )))?
            }
            None => Err(Status::permission_denied(format!(
                "No signature provided for public key {}",
                hex::encode(public_key_hash)
            ))),
        }
    }
}