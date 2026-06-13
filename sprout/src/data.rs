//! Chart data model (pure, no gpui).

/// One categorical bar: a label and a non-negative value.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub label: String,
    pub value: f64,
}
