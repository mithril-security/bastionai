use bastionai_learning::serialization::SizedObjectsBytes;
use std::convert::TryInto;
use std::sync::{Arc, RwLock};
use tch::TchError;

use crate::access_control::License;
use crate::remote_torch::ClientInfo;

/// Stored object with name, description and owner key
#[derive(Debug)]
pub struct Artifact<T> {
    pub data: Arc<RwLock<T>>,
    pub name: String,
    pub description: String,
    pub license: License,
    pub meta: Vec<u8>,
    pub client_info: Option<ClientInfo>,
}

impl<T> Artifact<T>
where
    for<'a> &'a T: TryInto<SizedObjectsBytes, Error = TchError>,
{
    /// Serializes the contained object and returns a new artifact that contains
    /// a SizedObjectBytes (binary buffer) instead of the object.
    ///
    /// Note that the object should be convertible into a SizedObjectBytes (with `TryInto`).
    pub fn serialize(&self) -> Result<Artifact<SizedObjectsBytes>, TchError> {
        Ok(Artifact {
            data: Arc::new(RwLock::new((&*self.data.read().unwrap()).try_into()?)),
            name: self.name.clone(),
            description: self.description.clone(),
            license: self.license.clone(),
            meta: self.meta.clone(),
            client_info: self.client_info.clone(),
        })
    }
}

impl Artifact<SizedObjectsBytes> {
    /// Deserializes the contained [`SizedObjectBytes`] object (binary buffer) and returns a
    /// new artifact that contains the deserialized object instead.
    ///
    /// Note that the object should be convertible from a SizedObjectBytes (with `TryForm`).
    pub fn deserialize<T: TryFrom<SizedObjectsBytes, Error = TchError> + std::fmt::Debug>(
        self,
    ) -> Result<Artifact<T>, TchError> {
        Ok(Artifact {
            data: Arc::new(RwLock::new(T::try_from(
                Arc::try_unwrap(self.data).unwrap().into_inner().unwrap(),
            )?)),
            name: self.name,
            description: self.description,
            license: self.license,
            meta: self.meta,
            client_info: self.client_info,
        })
    }
}
