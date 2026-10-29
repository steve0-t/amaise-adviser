//! Strict JSON parsing and the JSON Schema vocabulary used by the bundled contracts.
use crate::{ApiError, Result};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate fields")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| Strict(Value::Number(v)))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut out = Vec::new();
                while let Some(Strict(v)) = a.next_element()? {
                    out.push(v);
                }
                Ok(Strict(out.into()))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut out = Map::new();
                while let Some((k, Strict(v))) = a.next_entry::<String, Strict>()? {
                    if out.insert(k, v).is_some() {
                        return Err(de::Error::custom("duplicate field"));
                    }
                }
                Ok(Strict(out.into()))
            }
        }
        d.deserialize_any(V)
    }
}
pub fn parse(data: &[u8]) -> Result<Value> {
    serde_json::from_slice::<Strict>(data)
        .map(|v| v.0)
        .map_err(|_| {
            ApiError::new(
                400,
                "invalid_json",
                "Body must be valid UTF-8 JSON without duplicate fields.",
            )
        })
}
pub fn validate(value: &Value, schema: &Value) -> Result<()> {
    check(value, schema, schema, "$")
}
fn check(v: &Value, s: &Value, root: &Value, path: &str) -> Result<()> {
    let reject = |msg: &str| ApiError::new(400, "validation_error", format!("{path}: {msg}"));
    if s == &Value::Bool(false) {
        return Err(reject("value is not permitted"));
    }
    if let Some(r) = s["$ref"].as_str() {
        check(
            v,
            root.pointer(&r[1..]).expect("local schema reference"),
            root,
            path,
        )?;
    }
    if let Some(branches) = s["allOf"].as_array() {
        for b in branches {
            check(v, b, root, path)?;
        }
    }
    for k in ["oneOf", "anyOf"] {
        if let Some(bs) = s[k].as_array() {
            let n = bs
                .iter()
                .filter(|b| check(v, b, root, path).is_ok())
                .count();
            if n == 0 || (k == "oneOf" && n != 1) {
                return Err(reject("no matching schema variant"));
            }
        }
    }
    if s.get("const").is_some_and(|c| c != v)
        || s["enum"].as_array().is_some_and(|a| !a.contains(v))
    {
        return Err(reject("unexpected value"));
    }
    let matches = |t: &str| match t {
        "null" => v.is_null(),
        "boolean" => v.is_boolean(),
        "integer" => v.as_i64().is_some(),
        "number" => v.is_number(),
        "string" => v.is_string(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        _ => false,
    };
    if let Some(t) = s.get("type") {
        let ok = if let Some(t) = t.as_str() {
            matches(t)
        } else {
            t.as_array()
                .unwrap()
                .iter()
                .any(|t| matches(t.as_str().unwrap()))
        };
        if !ok {
            return Err(reject(
                "incorrect type or integer outside signed 64-bit range",
            ));
        }
    }
    let bounded = |n: usize, min: &str, max: &str| {
        n >= s[min].as_u64().unwrap_or(0) as usize
            && n <= s[max].as_u64().unwrap_or(u64::MAX) as usize
    };
    if let Some(o) = v.as_object() {
        if !bounded(o.len(), "minProperties", "maxProperties") {
            return Err(reject("incorrect number of fields"));
        }
        if let Some(req) = s["required"].as_array() {
            for k in req {
                if !o.contains_key(k.as_str().unwrap()) {
                    return Err(reject("missing required field"));
                }
            }
        }
        for (k, v) in o {
            let child = s["properties"]
                .get(k)
                .or_else(|| s.get("additionalProperties"));
            if let Some(child) = child {
                check(v, child, root, &format!("{path}.{k}"))?;
            }
            // Even unconstrained policy arguments cannot contain out-of-range integers.
            signed_numbers(v).map_err(|_| reject("integer outside signed 64-bit range"))?;
        }
    }
    if let Some(a) = v.as_array() {
        if !bounded(a.len(), "minItems", "maxItems") {
            return Err(reject("incorrect number of items"));
        }
        if s["uniqueItems"] == true && a.iter().enumerate().any(|(i, x)| a[..i].contains(x)) {
            return Err(reject("items must be unique"));
        }
        if let Some(child) = s.get("items") {
            for (i, v) in a.iter().enumerate() {
                check(v, child, root, &format!("{path}[{i}]"))?;
            }
        }
    }
    if let Some(text) = v.as_str() {
        if !bounded(text.chars().count(), "minLength", "maxLength") {
            return Err(reject("incorrect string length"));
        }
        if let Some(p) = s["pattern"].as_str()
            && !regex::Regex::new(p)
                .expect("trusted pattern")
                .is_match(text)
        {
            return Err(reject("invalid text pattern"));
        }
        if s["format"] == "date-time" && chrono::DateTime::parse_from_rfc3339(text).is_err() {
            return Err(reject("expected an RFC 3339 date-time"));
        }
    }
    if let Some(n) = v.as_number() {
        if n.is_u64() && n.as_i64().is_none() {
            return Err(reject("integer outside signed 64-bit range"));
        }
        let n = n.as_f64().unwrap();
        if n < s["minimum"].as_f64().unwrap_or(f64::NEG_INFINITY)
            || n > s["maximum"].as_f64().unwrap_or(f64::INFINITY)
        {
            return Err(reject("number outside permitted range"));
        }
    }
    Ok(())
}
fn signed_numbers(v: &Value) -> std::result::Result<(), ()> {
    match v {
        Value::Number(n) if n.is_u64() && n.as_i64().is_none() => Err(()),
        Value::Array(a) => a.iter().try_for_each(signed_numbers),
        Value::Object(o) => o.values().try_for_each(signed_numbers),
        _ => Ok(()),
    }
}
