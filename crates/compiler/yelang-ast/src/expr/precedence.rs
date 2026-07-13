/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */

macro_rules! define_precedence {
    ($($variant:ident),+ $(,)?) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum Precedence {
            $($variant),+
        }

        impl Precedence {
            pub const fn all() -> &'static [Precedence] {
                &[
                    $(
                        Precedence::$variant
                    ),+
                ]
            }
        }
    }
}

impl Precedence {
    pub fn increment(self) -> Self {
        let curr = self as usize;
        let next = (curr + 1).min(Self::all().len() - 1);
        Self::all()[next]
    }
}

define_precedence!(
    None,       // 0    or Lowest
    Assignment, // 1    = += -=
    LogicalOr,  // 2    ||
    LogicalAnd, // 3    &&
    BitwiseOr,  // 4    |
    BitwiseXor, // 5    ^
    BitwiseAnd, // 6    &
    Equality,   // 7    == !=
    Comparison, // 8    < > <= >=
    Membership, // 9    IN, NOT IN operators
    BitShift,   // 10   << >>
    Range,      // 11   ..
    Term,       // 12   + -
    Factor,     // 13   * / %
    Exponent,   // 14   **
    Unary,      // 15   - ! ~
    Call,       // 16   () [] .
    Primary,    // 17
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Associativity {
    Left,
    Right,
    NonAssociative, // None
}

pub trait PrecedenceExt {
    fn precedence(&self) -> Precedence;

    fn associativity(&self) -> Associativity {
        match self.precedence() {
            Precedence::Assignment => Associativity::Right,
            Precedence::Exponent => Associativity::Right,
            Precedence::Unary => Associativity::Right,
            _ => Associativity::Left,
        }
    }

    // fn higher_than(self, other: Precedence) -> bool;
    // fn lower_than(self, other: Precedence) -> bool;
}
