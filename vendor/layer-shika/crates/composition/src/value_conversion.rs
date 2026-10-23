use layer_shika_adapters::platform::slint_interpreter::Value;

/// Trait for callback return types
pub trait IntoValue {
    fn into_value(self) -> Value;
}

impl IntoValue for () {
    fn into_value(self) -> Value {
        Value::Void
    }
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

impl IntoValue for i32 {
    fn into_value(self) -> Value {
        Value::Number(f64::from(self))
    }
}

impl IntoValue for f32 {
    fn into_value(self) -> Value {
        Value::Number(f64::from(self))
    }
}

impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::Number(self)
    }
}

impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::String(self.into())
    }
}

impl IntoValue for &str {
    fn into_value(self) -> Value {
        Value::String(self.into())
    }
}
