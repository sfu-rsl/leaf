use std::string::String;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, IntoDeserializer, Unexpected, Visitor, value},
};

use super::{
    pri::BasicBlockLocation,
    types::{DefId, InstanceKindId},
};

macro_rules! impl_from_str_through_des {
    ($t:ty) => {
        impl core::str::FromStr for $t {
            type Err = value::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::deserialize(s.into_deserializer())
            }
        }
    };
}

impl DefId {
    fn to_ser_str(&self) -> String {
        std::format!("{}:{}", self.0, self.1)
    }
}

impl Serialize for DefId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_ser_str())
    }
}

impl<'de> Deserialize<'de> for DefId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MyVisitor;
        impl<'de> Visitor<'de> for MyVisitor {
            type Value = DefId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string in ##:## format")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let parts = v
                    .split_once(':')
                    .ok_or(E::invalid_value(Unexpected::Str(v), &self))?;
                Ok(DefId(
                    parts.0.parse().map_err(E::custom)?,
                    parts.1.parse().map_err(E::custom)?,
                ))
            }
        }

        deserializer.deserialize_str(MyVisitor)
    }
}

impl_from_str_through_des!(DefId);

impl InstanceKindId {
    fn to_ser_str(&self) -> String {
        if core::hint::likely(self.0 == 0) {
            self.1.to_ser_str()
        } else {
            std::format!("{}-{}", self.1.to_ser_str(), self.0)
        }
    }
}

impl Serialize for InstanceKindId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_ser_str())
    }
}

impl<'de> Deserialize<'de> for InstanceKindId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MyVisitor;
        impl<'de> Visitor<'de> for MyVisitor {
            type Value = InstanceKindId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string in ##:##(-##)? format")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if let Some(parts) = v.split_once('-') {
                    core::hint::cold_path();
                    Ok(InstanceKindId(
                        parts.1.parse().map_err(E::custom)?,
                        parts.0.parse().map_err(E::custom)?,
                    ))
                } else {
                    Ok(InstanceKindId(0, v.parse().map_err(E::custom)?))
                }
            }
        }

        deserializer.deserialize_str(MyVisitor)
    }
}

impl_from_str_through_des!(InstanceKindId);

impl Serialize for BasicBlockLocation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&std::format!("{}:{}", self.body.to_ser_str(), self.index))
    }
}

impl<'de> Deserialize<'de> for BasicBlockLocation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MyVisitor;
        impl<'de> Visitor<'de> for MyVisitor {
            type Value = BasicBlockLocation;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string in ##:##(-##)?:## format")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let parts = v.split_at(
                    v.rfind(':')
                        .ok_or(E::invalid_value(Unexpected::Str(v), &self))?,
                );
                Ok(BasicBlockLocation {
                    body: parts.0.parse().map_err(E::custom)?,
                    index: parts.1[1..].parse().map_err(E::custom)?,
                })
            }
        }

        deserializer.deserialize_str(MyVisitor)
    }
}

impl_from_str_through_des!(BasicBlockLocation);
