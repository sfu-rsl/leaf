use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Unexpected, Visitor},
};

use super::IntType;

impl Serialize for IntType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for IntType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MyVisitor;
        impl<'de> Visitor<'de> for MyVisitor {
            type Value = IntType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string in `(i|u)\\d+` format")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v.len() < 2 {
                    return Err(E::invalid_value(Unexpected::Str(v), &self));
                }

                let is_signed = match v.as_bytes()[0] {
                    b'i' => true,
                    b'u' => false,
                    _ => return Err(E::invalid_value(Unexpected::Str(v), &self)),
                };

                Ok(IntType {
                    is_signed,
                    bit_size: v[1..].parse().map_err(E::custom)?,
                })
            }
        }

        deserializer.deserialize_str(MyVisitor)
    }
}
