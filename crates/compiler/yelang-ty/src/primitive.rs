/*! Primitive type enumerations. */

/// Signed integer types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
}

/// Unsigned integer types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UintTy {
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

/// Floating-point types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FloatTy {
    F32,
    F64,
}

impl IntTy {
    pub fn name_str(self) -> &'static str {
        match self {
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::I128 => "i128",
            IntTy::Isize => "isize",
        }
    }

    pub fn size_bits(self, ptr_size: u64) -> u64 {
        match self {
            IntTy::I8 => 8,
            IntTy::I16 => 16,
            IntTy::I32 => 32,
            IntTy::I64 => 64,
            IntTy::I128 => 128,
            IntTy::Isize => ptr_size,
        }
    }
}

impl UintTy {
    pub fn name_str(self) -> &'static str {
        match self {
            UintTy::U8 => "u8",
            UintTy::U16 => "u16",
            UintTy::U32 => "u32",
            UintTy::U64 => "u64",
            UintTy::U128 => "u128",
            UintTy::Usize => "usize",
        }
    }

    pub fn size_bits(self, ptr_size: u64) -> u64 {
        match self {
            UintTy::U8 => 8,
            UintTy::U16 => 16,
            UintTy::U32 => 32,
            UintTy::U64 => 64,
            UintTy::U128 => 128,
            UintTy::Usize => ptr_size,
        }
    }
}

impl FloatTy {
    pub fn name_str(self) -> &'static str {
        match self {
            FloatTy::F32 => "f32",
            FloatTy::F64 => "f64",
        }
    }

    pub fn size_bits(self) -> u64 {
        match self {
            FloatTy::F32 => 32,
            FloatTy::F64 => 64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_ty_names() {
        assert_eq!(IntTy::I32.name_str(), "i32");
        assert_eq!(IntTy::Isize.name_str(), "isize");
    }

    #[test]
    fn uint_ty_sizes() {
        assert_eq!(UintTy::U8.size_bits(64), 8);
        assert_eq!(UintTy::Usize.size_bits(64), 64);
        assert_eq!(UintTy::Usize.size_bits(32), 32);
    }

    #[test]
    fn float_ty_sizes() {
        assert_eq!(FloatTy::F32.size_bits(), 32);
        assert_eq!(FloatTy::F64.size_bits(), 64);
    }
}
