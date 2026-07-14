//! Primitive types and operations for Yelang.
//!
//! Mirrors Rust's core primitive types: integers, floats, bool, char, unit.

/// Marker trait for all primitive types.
pub trait Primitive {}

impl Primitive for i8 {}
impl Primitive for i16 {}
impl Primitive for i32 {}
impl Primitive for i64 {}
impl Primitive for i128 {}
impl Primitive for isize {}
impl Primitive for u8 {}
impl Primitive for u16 {}
impl Primitive for u32 {}
impl Primitive for u64 {}
impl Primitive for u128 {}
impl Primitive for usize {}
impl Primitive for f32 {}
impl Primitive for f64 {}
impl Primitive for bool {}
impl Primitive for char {}
impl Primitive for () {}
