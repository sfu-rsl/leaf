// We serialize it as string for map key compatibility.

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, IntoDeserializer, Unexpected, Visitor, value},
};

use crate::pri::BasicBlockLocation;

use super::types::DefId;

impl Serialize for DefId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&std::format!("{}:{}", self.0, self.1))
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

impl std::str::FromStr for DefId {
    type Err = value::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::deserialize(s.into_deserializer())
    }
}

impl Serialize for BasicBlockLocation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&std::format!(
            "{}:{}:{}",
            self.body.0,
            self.body.1,
            self.index
        ))
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
                formatter.write_str("a string in ##:##:## format")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let mut parts = v.splitn(3, ':');
                let parts = parts
                    .next()
                    .zip(parts.next())
                    .zip(parts.next())
                    .ok_or(E::invalid_value(Unexpected::Str(v), &self))?;
                let parts = (parts.0.0, parts.0.1, parts.1);
                Ok(BasicBlockLocation {
                    body: DefId(
                        parts.0.parse().map_err(E::custom)?,
                        parts.1.parse().map_err(E::custom)?,
                    ),
                    index: parts.2.parse().map_err(E::custom)?,
                })
            }
        }

        deserializer.deserialize_str(MyVisitor)
    }
}

impl std::str::FromStr for BasicBlockLocation {
    type Err = value::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::deserialize(s.into_deserializer())
    }
}
