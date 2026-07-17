/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use crate::Symbol;
use crate::{
    Array, ArrayAccess, AssignEqExpr, AssignOpExpr, AsyncExpr, BinaryExpr, BindAtExpr, BlockExpr,
    BreakExpr, CallExpr, ComprehensionExpr, ContinueExpr, DestructureAssignExpr, DocumentAccess,
    ForLoopExpr, GroupedExpr, IfExpr, IsTypeExpr, LambdaExpr, LetExpr, Literal, LoopExpr,
    MatchExpr, MemberAccess, MethodCallExpr, Object, Path, Query, RangeExpr, StructExpr,
    TernaryExpr, TrySafeAccess, TypeAscription, TypeCast, UnaryExpr, WhileExpr,
};
use yelang_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    /// Literal string part: `"hello"`
    Literal(Symbol),
    /// Expression part: `${expr}`
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    // ===== CORE EXPRESSIONS =====
    // Atomic expressions
    /// Literal values: `42`, `"hello"`, `true`
    ///
    /// # Example
    /// ```
    /// 42
    /// "hello"
    /// true
    /// ```
    Literal(Literal),

    /// Interpolated string: `"Hello ${name}"`
    ///
    /// # Example
    /// ```
    /// "Hello ${name}"
    /// "Value: ${x + 1}"
    /// ```
    InterpolatedString(Vec<StringPart>),

    /// Variable/constant reference: `x`, `std::collections::HashMap`
    ///
    /// # Example
    /// ```
    /// x
    /// user.name
    /// std::collections::HashMap
    /// ```
    Path(Path),

    /// The underscore pattern (`_`), used for ignoring values and types.
    ///
    /// Primarily appears in patterns for destructuring and match expressions,
    /// but also in limited expression contexts for type inference.
    ///
    /// # Examples
    ///
    /// ```rust
    /// // Pattern usage
    /// let (x, _, z) = (1, 2, 3);
    ///
    /// // Match expression
    /// match Some(42) {
    ///     Some(_) => println!("has value"),
    ///     None => (),
    /// }
    ///
    /// // Type inference
    /// let count: _ = 42_u32;
    /// let parsed = "42".parse::<_>();
    /// ```
    ///
    /// # Note
    /// Cannot be assigned to: `_ = value` is invalid.
    Underscore,

    // Operations
    /// Binary operators: `a + b`, `x == y`, `flag && other`
    ///
    /// # Example
    /// ```
    /// 1 + 2
    /// x == y
    /// flag && is_valid
    /// ```
    Binary(BinaryExpr),

    /// Unary operators: `-x`, `!flag`, `*ptr`
    ///
    /// # Example
    /// ```
    /// -x
    /// !enabled
    /// *pointer
    /// ```
    Unary(UnaryExpr),

    /// AssignEq expressions: `a = b`
    ///
    /// # Example
    /// ```
    /// x = 42
    /// user.name = "Alice"
    /// ```
    AssignEq(AssignEqExpr),

    /// AssignOp expressions: `a += b`, `count *= 2`
    ///
    /// # Example
    /// ```
    /// x += 1
    /// total *= factor
    /// ```
    AssignOp(AssignOpExpr),

    /// Structural destructuring assignment: `{ value: user } = row`, `(x, y) = pair`.
    ///
    /// Unlike `let` patterns, bindings inside this pattern must resolve to existing mutable
    /// places. The assignment does not introduce new locals.
    DestructureAssign(DestructureAssignExpr),

    /// Try (`?`) operator for `Result`/`Option`-style control flow: `value?`
    ///
    /// # Example
    /// ```
    /// let result = read_file()?;
    /// ```
    Try(TrySafeAccess),

    // ===== CONTROL FLOW =====
    /// If expressions: `if condition { then } else { else }`
    ///
    /// # Example
    /// ```
    /// if x > 0 {
    ///     positive()
    /// } else {
    ///     negative()
    /// }
    /// ```
    If(IfExpr),

    /// Let expression for pattern matching in conditions: `let Some(x) = opt`
    ///
    /// Used in if-let chains and while-let conditions. When combined with `&&`,
    /// creates let-chains. Has lower precedence than most operators.
    ///
    /// # Example
    /// ```
    /// // In if condition
    /// if let Some(x) = opt && x > 5 {
    ///     println!("{}", x);
    /// }
    ///
    /// // In while condition
    /// while let Some(item) = iter.next() {
    ///     process(item);
    /// }
    ///
    /// // Let-chains (multiple let expressions with &&)
    /// if let Some(x) = opt && let Ok(y) = res && x == y {
    ///     // both patterns matched and condition true
    /// }
    /// ```
    Let(LetExpr),

    /// Match expressions with pattern matching
    ///
    /// # Example
    /// ```
    /// match value {
    ///     Some(x) if x > 0 => positive(x),
    ///     Some(x) => negative(x),
    ///     None => default(),
    /// }
    /// ```
    Match(Box<MatchExpr>),

    /// Ternary conditional: `condition ? then : else`
    ///
    /// # Example
    /// ```
    /// x > 0 ? "positive" : "negative"
    /// ```
    // FIXME: Decide whether or not to support ternary. Most likely not.
    Ternary(TernaryExpr),

    /// Loop expressions: `loop { break value }`
    ///
    /// Infinite loops that can return values via break
    ///
    /// # Example
    /// ```
    /// let result = loop {
    ///     if condition {
    ///         break 42;
    ///     }
    /// };
    /// ```
    Loop(Box<LoopExpr>),

    /// While loop expressions: `while condition { body }`
    ///
    /// A conditional loop that continues executing as long as the condition evaluates to true.
    ///
    /// # Example
    /// ```
    /// while x < 10 {
    ///     x += 1;
    /// }
    /// ```
    While(WhileExpr),

    /// For loop expressions: `for pat in iter { body }`
    ///
    /// Iterates over elements of an iterator, binding each element to a pattern.
    ///
    /// # Example
    /// ```
    /// for item in items {
    ///     process(item);
    /// }
    ///
    /// for (key, value) in map {
    ///     println!("{}: {}", key, value);
    /// }
    /// ```
    ForLoop(ForLoopExpr),

    /// Break expressions: `break`, `break value`, `break 'label`, or `break 'label value`
    ///
    /// Exits a loop, optionally targeting a specific labeled loop,
    /// with an optional return value.
    /// Has type `!` (never) as it never returns normally.
    ///
    /// # Example
    /// ```
    /// loop {
    ///     if done {
    ///         break;  // exit loop
    ///     }
    ///     if found {
    ///         break result;  // exit with value
    ///     }
    /// }
    /// 'outer: loop {
    ///     loop {
    ///         break 'outer;  // exit outer loop
    ///     }
    /// }
    /// ```
    Break(BreakExpr),

    /// Continue expression: `continue` or `continue 'label`
    ///
    /// Skips to the next iteration of a loop, optionally targeting
    /// a specific labeled loop.
    /// Has type `!` (never) as it never returns normally.
    ///
    /// # Example
    /// ```
    /// loop {
    ///     if should_skip {
    ///         continue;  // next iteration
    ///     }
    ///     process_item();
    /// }
    /// 'outer: loop {
    ///     loop {
    ///         continue 'outer;  // continue outer loop
    ///     }
    /// }
    /// ```
    Continue(ContinueExpr),

    /// Return expressions: `return`, `return value`
    ///
    /// Returns from the current function with an optional value.
    /// Has type `!` (never) as it never returns normally.
    ///
    /// # Example
    /// ```
    /// fn example() -> i32 {
    ///     if condition {
    ///         return 42;  // early return
    ///     }
    ///     0  // implicit return
    /// }
    /// ```
    Return(Option<Box<Expr>>),

    // Type operations
    /// Type cast: `x as i32`
    ///
    /// # Example
    /// ```
    /// let x = 42.5 as i32;
    /// ```
    TypeCast(TypeCast),

    /// Type ascription: `value: Type`
    TypeAscription(TypeAscription),

    /// Type check: `x is string`
    ///
    /// # Example
    /// ```
    /// if value is string {
    ///     print(value)
    /// }
    /// ```
    IsType(IsTypeExpr),

    // ===== STRUCTURED DATA CONSTRUCTION =====
    /// Struct literal construction
    ///
    /// # Example
    /// ```
    /// User { id: 1, name: "John" }
    /// path::User { id: 1, name: "John", ..rest }
    /// ```
    Struct(StructExpr),

    // ===== COLLECTIONS =====
    /// Array literals: `[1, 2, 3]`
    ///
    /// # Example
    /// ```
    /// [1, 2, 3]
    /// [0; 10]  // array of 10 zeros
    /// ```
    Array(Array),

    /// Object literals: `{ x: 1, y: 2 }`
    ///
    /// # Example
    /// ```
    /// { name: "Alice", age: 30 }
    /// ```
    Object(Object),

    /// Tuple literals: `(1, "hello")`
    ///
    /// # Example
    /// ```
    /// (1, "hello")
    /// (x, y, z)
    /// ```
    Tuple(Vec<Expr>),

    /// Range expressions: `1..10`, `..5`, `1..=10`
    ///
    /// # Example
    /// ```
    /// 1..10       // exclusive end
    /// 1..=10      // inclusive end
    /// ..5         // up to 5
    /// 5..         // from 5 onwards
    /// ```
    Range(RangeExpr),

    /// Comprehensions: `[x * 2 for x in items if x > 0]`
    ///
    /// # Example
    /// ```
    /// [x * 2 for x in items if x > 0]
    /// items.map(|x| x * 2).filter(|x| x > 0)
    /// [user.name for user in users]
    /// ```
    Comprehension(ComprehensionExpr),

    // Access patterns
    /// Field access: `obj.field`
    ///
    /// # Example
    /// ```
    /// user.name
    /// point.x
    /// ```
    MemberAccess(MemberAccess),

    /// Array indexing: `arr[i]`
    ///
    /// # Example
    /// ```
    /// arr[0]
    /// matrix[i][j]
    /// ```
    ArrayAccess(ArrayAccess),

    /// Document access: `users[*].{name, age: 123}`, `info.{name, age}`
    ///
    /// Database-specific projection syntax
    ///
    /// # Example
    /// ```
    /// users[*].{name, age}
    /// info.{name, age: user.age}
    /// ```
    DocumentAccess(DocumentAccess),

    /// Bind-at expression: `users@u`
    ///
    /// Binds a result to an alias for later reference
    ///
    /// # Example
    /// ```
    /// users@u
    /// SELECT * FROM users@u WHERE u.age > 18
    /// ```
    BindAt(BindAtExpr),

    // Calls
    /// Function call: `foo(1, 2)`
    ///
    /// # Example
    /// ```
    /// add(1, 2)
    /// println("hello")
    /// ```
    /// Enum variant construction
    ///
    /// # Example
    /// ```
    /// Option::Some(42)
    /// Result::Ok("success")
    /// Status::Active
    /// ```
    Call(CallExpr),

    /// Method call: `obj.method(arg)`
    ///
    /// # Example
    /// ```
    /// list.push(item)
    /// string.to_uppercase()
    /// ```
    MethodCall(MethodCallExpr),

    /// Lambda expressions: `|x| x + 1`
    ///
    /// # Example
    /// ```
    /// |x| x + 1
    /// |a, b| a + b
    /// async |x| { await process(x) }
    /// ```
    Lambda(LambdaExpr),

    // Blocks and scoping
    /// Block expressions: `{ stmt; expr }`
    ///
    /// # Example
    /// ```
    /// {
    ///     let x = 42;
    ///     x + 1
    /// }
    /// ```
    Block(BlockExpr),

    // Graph queries
    /// Query expressions: `SELECT ... FROM ... WHERE ...`
    ///
    /// Database query as a first-class expression
    ///
    /// # Example
    /// ```
    /// SELECT * FROM users WHERE age > 18
    /// SELECT name, age FROM users ORDER BY age DESC
    /// ```
    Query(Box<Query>),

    // Grouping
    /// Grouped expression for precedence: `(expr)`
    ///
    /// # Example
    /// ```
    /// (a + b) * c
    /// ```
    Grouped(GroupedExpr),

    // ===== ERROR RECOVERY =====
    /// Error placeholder for parser recovery
    ///
    /// Used when parsing fails but we want to continue parsing
    Err,

    /// Dummy/placeholder expression
    ///
    /// Used as a placeholder in various contexts
    Dummy,

    // ===== CONCURRENCY =====
    /// Async block/function
    ///
    /// # Example
    /// ```
    /// async { await fetch_data() }
    /// async fn process() { ... }
    /// ```
    Async(AsyncExpr),

    /// Generator block
    ///
    /// # Example
    /// ```
    /// gen { yield 1; yield 2; }
    /// ```
    Gen(Box<Expr>),

    /// Await expressions: `async_fn().await`
    ///
    /// # Example
    /// ```
    /// let result = fetch_data().await;
    /// ```
    Await(Box<Expr>),
}

/// Restrictions for parsing expressions.
///
/// Similar to rust-analyzer's `Restrictions`, this allows context-specific
/// parsing behavior to be threaded through the expression parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restrictions {
    /// If true, struct literals are forbidden in this context.
    ///
    /// Used in:
    /// - If/while conditions: prevents `if opt { ... }` from parsing as `if (opt { ... })`
    /// - Let expression RHS: prevents `let x = opt { ... }` from parsing the `{ ... }` as struct literal
    /// - After `..` in ranges when followed by `{`: prevents `x..{ }` from treating `{ }` as struct
    pub forbid_structs: bool,

    /// If true, treat `>` as a delimiter (not an infix operator).
    ///
    /// This is used when parsing expressions inside angle-bracketed generic arguments, where
    /// `>` closes the argument list and should not be parsed as a comparison operator.
    pub gt_is_delimiter: bool,
}

impl Default for Restrictions {
    fn default() -> Self {
        Self {
            forbid_structs: false,
            gt_is_delimiter: false,
        }
    }
}

impl Restrictions {
    /// No restrictions - struct literals allowed
    pub const NONE: Self = Self {
        forbid_structs: false,
        gt_is_delimiter: false,
    };

    /// Forbid struct literals (for if/while conditions and let expressions)
    pub const NO_STRUCT: Self = Self {
        forbid_structs: true,
        gt_is_delimiter: false,
    };

    /// Parse expressions inside `<...>` generic argument lists.
    pub const GENERIC_ARG: Self = Self {
        forbid_structs: false,
        gt_is_delimiter: true,
    };
}
