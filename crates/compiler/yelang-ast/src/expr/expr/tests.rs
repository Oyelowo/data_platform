use super::*;
use yelang_interner::Interner;
use yelang_lexer::{Literal, TokenKind};

#[test]
fn test_parse_expr() {
    let input = "1 * (2 + 3)";
    let input = "user[0].settings[1..5].mode[*].age + 3";
    let input = "((age)[Where lowox + 9 > 4].settings[1..5].mode[*].age + 3 * (age(23, 'lowo') > 5)  + 4 )+ 3";
    let input = "(age(23, 'lowo'))";
    let input = "age[where true].mmx[0](23, 'lowo')[4..9]";
    let input = "adf + 3";
    let input = "(user[0].settings[1..5].mode[*].age + 3 * (age(23, 'lowo') > 5)  + 4 )+ 3";
    let input = "user[Where name + 2 > 5][4..9].settings[1..5].mode[*].age";
    // let input = "[1 ]";
    // let input = "!products[WHERE (price * -1.1) and cost + -5]";
    // let input = "[0]";
    // let input = "age[0]";
    // let input = "![4,5, {name: 'xdf'}]";
    // let input = "-----34.4";
    // let input = "lowo.age.time.place";
    let input = "lowo.{name, age: user.age}";
    let input = "
user[*].{
  user_id: user.id,
  blogs_written: blog[*].{
    title: blog.title,
    comments: comment[*].{
      commenter: comment.user_id,
      comment_text: comment.text
    }
  },
  books_written: books[*].{
    title: book.title,
    reviews: review[where age > 5 and !true].{
      reviewer_name: review.user_id,
      review_content: review.content
    }
  }
}
";
    let x = !!!---0;
    let input = "
users@uxx[*].{
  id,
  name,
    // blogs: u.blogs@b[where b.date > '2023-01-01'][order by b.views desc][0..1].{
    blogs: uxx.blogs@bxx[where bxx.date > dt'2023-01-01'][0..1].{
    // title: 45 as int,
    title: koko::manaa::lowo() as int,
    first_tag: bxx.info.tags@txx[where txx.kind == 'tech'][0].name,
    matching_comments: bxx.comments@cxx[where cxx.user == uxx.name][*].text
  }
}
";
    // let input = "users@xx";
    // let input = "users as int";
    // let input = "$users.man()@xx";

    // let input = "34.4";
    // let input = "(2+2).age";
    // let input = "2 +2 and !!!-----34.4";
    // let input = "[1, {name: user[Where name + 2 > 5].settings[1:5].mode[*].age } ]";
    // let input = "{name: user[Where name + 2 > 5][4:9].settings[1:5].mode[*].age }.name";
    // let tokens = Token::tokenize(input).unwrap();
    // let mut tokens = TokenKind::tokenize(input).unwrap();
    // panic!("{:#?}", tokens);
    // let ast = tokens
    //     .parse::<Expr>()
    //     .inspect(|t| {
    //         panic!("loowow{:#?}", t);
    //     })
    //     .inspect_err(|e| {
    //         panic!("errorrr{:#?}", e);
    //     });
    // .unwrap();
    // println!("{:?}", ast);
}

#[test]
fn test_visit_expr() {
    let input = "1 * (2 + 3)";
    let input = "user[0].settings[1..5].mode[*].age + 3";
    let input = "((age)[Where lowox + 9 > 4].settings[1..5].mode[*].age + 3 * (age(23, 'lowo') > 5)  + 4 )+ 3";
    let input = "(age(23, 'lowo'))";
    let input = "age[where true].mmx[0](23, 'lowo')[4..9]";
    let input = "adf + 3";
    let input = "(user[0].settings[1..5].mode[*].age + 3 * (age(23, 'lowo') > 5)  + 4 )+ 3";
    let input = "user[Where name + 2 > 5][4..9].settings[1..5].mode[*].age";
    // let input = "[1 ]";
    // let input = "!products[WHERE (price * -1.1) and cost + -5]";
    // let input = "[0]";
    // let input = "age[0]";
    // let input = "![4,5, {name: 'xdf'}]";
    // let input = "-----34.4";
    // let input = "lowo.age.time.place";
    // let input = "lowo.{name, age: user.papa}";
    let input = "lowo(name, koko)";
    let input = "lowo(name, koko)";
    let input = "select {
            name: 1,
            age: 9,
        } from user:User;";
    let input = "
SELECT users@u[*].{
  user_id: u.id,
  blogs: u.blog[Where age + 89].{
    title
  }
}
from users:User
links
  (user) -> [writes:WritesBlog where writes.published_date > dt'2020-01-01'] -> (blog:Blog where blog.views > 10000),
  (user) -> [writes:WritesBlog where writes.published_date < dt'2020-01-01'] -> (blog:Blog where blog.views > 10000) -> [comments:Comment] -> (commenter:Comment where comment.likes > 1000)

where user[0..6].age[2] > 30
;

";

    // let input = "lowo.age";
    // let mut tokens = TokenKind::tokenize(input).unwrap();
    // // let ast = tokens.parse::<Expr>().unwrap();
    // let ast = tokens.parse::<Stmt>().unwrap();
    //
    // #[derive(Debug)]
    // struct VisitObjectValue {
    //     base: Vec<String>,
    //     visited: Vec<String>,
    //     keys: Vec<String>,
    //     ops: Vec<String>,
    // }
    //
    // impl Visitor for VisitObjectValue {
    //     // fn visit_test_remove(&mut self, cha: &Ident) -> ControlFlow<()> {
    //     //     // panic!("Chama LinksClause accept called with {} paths", 99);
    //     //     self.keys.push(cha.to_string());
    //     //     ControlFlow::Continue(())
    //     // }
    //
    //     fn visit_ident(&mut self, ident: &Ident) -> ControlFlow<()> {
    //         self.keys.push(ident.to_string());
    //         ControlFlow::Continue(())
    //     }
    //
    //     fn visit_binary(&mut self, binary: &BinaryExpr) -> ControlFlow<()> {
    //         self.ops.push(binary.op.to_string());
    //         binary.accept(self)
    //     }
    // }
    //
    // let mut visitor = VisitObjectValue {
    //     base: Vec::new(),
    //     visited: Vec::new(),
    //     keys: Vec::new(),
    //     ops: Vec::new(),
    // };
    // ast.accept(&mut visitor);
    // assert_eq!(
    //     visitor.keys,
    //     vec![
    //         "user",
    //         "user_id",
    //         "user",
    //         "id",
    //         "blogs",
    //         "blog",
    //         "age",
    //         "title",
    //         "title",
    //         "user",
    //         "user",
    //         "writes",
    //         "WritesBlog",
    //         "writes",
    //         "writes",
    //         "published_date",
    //         "blog",
    //         "Blog",
    //         "blog",
    //         "blog",
    //         "views",
    //         "user",
    //         "user",
    //         "writes",
    //         "WritesBlog",
    //         "writes",
    //         "writes",
    //         "published_date",
    //         "blog",
    //         "Blog",
    //         "blog",
    //         "blog",
    //         "views",
    //         "comments",
    //         "Comment",
    //         "comments",
    //         "commenter",
    //         "Comment",
    //         "commenter",
    //         "comment",
    //         "likes",
    //         "user",
    //         "age"
    //     ]
    // );
    // assert_eq!(visitor.ops, vec!["+", ">", ">", "<", ">", ">", ">"]);
    // assert_eq!(visitor.base, vec!["lowo", "user"]);
    // assert_eq!(visitor.visited, vec!["name", "age"]);
}

#[test]
fn parse_intrinsic_expr() {
    let input = r#"@intrinsic("x", 1, 2)"#;
    let interner = Interner::new();
    let mut stream = TokenKind::tokenize(input, &interner).expect("tokenize");
    let expr = stream.parse::<Expr>().expect("parse expr");

    let ExprKind::Intrinsic(intrinsic) = expr.kind else {
        panic!("expected intrinsic expression, got {:?}", expr.kind);
    };
    assert_eq!(
        interner.resolve(&intrinsic.name.symbol),
        "intrinsic",
        "expected intrinsic namespace"
    );
    assert_eq!(intrinsic.args.len(), 3, "expected three arguments");

    let first = &intrinsic.args[0];
    match &first.kind {
        ExprKind::Literal(Literal::Str(s)) => {
            assert_eq!(
                interner.resolve(&s.value),
                "x",
                "first argument should be string literal 'x'"
            );
        }
        _ => panic!("first argument should be string literal"),
    }
    assert!(
        matches!(&intrinsic.args[1].kind, ExprKind::Literal(Literal::Int(_))),
        "second argument should be integer literal"
    );
    assert!(
        matches!(&intrinsic.args[2].kind, ExprKind::Literal(Literal::Int(_))),
        "third argument should be integer literal"
    );
}
