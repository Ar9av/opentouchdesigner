//! The value type carried by parameters.
//!
//! Deliberately small: parameters are an artist-facing surface, not a general
//! purpose type system (see PLAN.md §2 — "keep the families", don't grow a
//! language).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Float(f64),
    Int(i64),
    Bool(bool),
    Str(String),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Float(_) => "float",
            Value::Int(_) => "int",
            Value::Bool(_) => "bool",
            Value::Str(_) => "str",
            Value::Vec2(_) => "vec2",
            Value::Vec3(_) => "vec3",
            Value::Vec4(_) => "vec4",
        }
    }

    /// Scalar coercion. Vectors yield their first component, strings parse or
    /// fall back to 0.0 — matching the permissiveness artists expect from TD.
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Float(v) => *v,
            Value::Int(v) => *v as f64,
            Value::Bool(v) => {
                if *v {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
            Value::Vec2(v) => v[0],
            Value::Vec3(v) => v[0],
            Value::Vec4(v) => v[0],
        }
    }

    pub fn as_f32(&self) -> f32 {
        self.as_f64() as f32
    }

    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            other => other.as_f64().round() as i64,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            Value::Str(s) => !s.is_empty() && s != "0" && s != "false",
            other => other.as_f64() != 0.0,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Float(v) => v.to_string(),
            Value::Int(v) => v.to_string(),
            Value::Bool(v) => v.to_string(),
            Value::Vec2(v) => format!("{} {}", v[0], v[1]),
            Value::Vec3(v) => format!("{} {} {}", v[0], v[1], v[2]),
            Value::Vec4(v) => format!("{} {} {} {}", v[0], v[1], v[2], v[3]),
        }
    }

    /// Widen to RGBA for the GPU. Scalars broadcast to RGB with alpha 1.
    pub fn as_vec4_f32(&self) -> [f32; 4] {
        match self {
            Value::Vec4(v) => [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32],
            Value::Vec3(v) => [v[0] as f32, v[1] as f32, v[2] as f32, 1.0],
            Value::Vec2(v) => [v[0] as f32, v[1] as f32, 0.0, 1.0],
            other => {
                let s = other.as_f32();
                [s, s, s, 1.0]
            }
        }
    }

    /// Whether `other` can replace this value without changing the parameter's
    /// declared type. Used to keep loaded projects from corrupting a node.
    pub fn same_type_as(&self, other: &Value) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// Build a value of *this* value's type from a plain float. Used when an
    /// expression or a CHOP export drives a typed parameter.
    pub fn coerce_from_f64(&self, v: f64) -> Value {
        match self {
            Value::Float(_) => Value::Float(v),
            Value::Int(_) => Value::Int(v.round() as i64),
            Value::Bool(_) => Value::Bool(v != 0.0),
            Value::Str(_) => Value::Str(v.to_string()),
            Value::Vec2(_) => Value::Vec2([v; 2]),
            Value::Vec3(_) => Value::Vec3([v; 3]),
            Value::Vec4(_) => Value::Vec4([v; 4]),
        }
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v as f64)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v as i64)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl From<[f64; 3]> for Value {
    fn from(v: [f64; 3]) -> Self {
        Value::Vec3(v)
    }
}
impl From<[f64; 4]> for Value {
    fn from(v: [f64; 4]) -> Self {
        Value::Vec4(v)
    }
}
