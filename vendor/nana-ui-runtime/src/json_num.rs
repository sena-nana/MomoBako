//! JSON number parsing shared by semantic attrs (gutter spans) and Terminal
//! `screen` cells. Host dumps mix integers and whole floats.

pub(crate) fn json_u64(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let number = value.as_f64()?;
        if number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= u64::MAX as f64
        {
            Some(number as u64)
        } else {
            None
        }
    })
}
