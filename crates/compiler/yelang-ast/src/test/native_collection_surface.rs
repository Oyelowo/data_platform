use crate::{Stmt, TokenKind};
use yelang_lexer::{All, ParseTokenStream};

fn parse_all<T: ParseTokenStream<TokenKind>>(src: &str) -> T {
    let mut interner = crate::Interner::new();
    let mut stream = TokenKind::tokenize(src, &mut interner).expect("tokenization failed");
    stream
        .parse::<All<T>>()
        .unwrap_or_else(|err| panic!("failed to parse `{src}`: {err:?}"))
        .into_inner()
}

#[test]
fn native_collection_method_surface_parses_as_ordinary_expressions() {
    for src in [
        "xs.len();",
        "xs.count();",
        "xs.is_empty();",
        "xs.any(|x| x > 0);",
        "xs.none(|x| x < 0);",
        "xs.all(|x| x >= 0);",
        "xs.sum();",
        "xs.min();",
        "xs.max();",
        "xs.avg();",
        "xs.slice(1..3);",
        "xs.range(1..=3);",
        "xs.skip(1).take(2);",
        "xs.at(0);",
        "xs.get(0);",
        "xs.first();",
        "xs.last();",
        "xs.exactly_one();",
        "xs.expect_one();",
        "xs.map(|x| x + 1);",
        "xs.filter(|x| x > 1);",
        "xs.filter_map(|x| Option::Some(x));",
        "xs.flat_map(|x| [x, x + 1]);",
        "xss.flatten();",
        "xs.compact();",
        "xs.order_by(|x| desc(x));",
        "xs.order_by(|x| order::asc(x));",
        "xs.order_by(|x| order::desc(x));",
        "xs.order_by_asc(|x| x);",
        "xs.order_by_desc(|x| x);",
        "xs.reversed();",
        "xs.enumerate();",
        "xs.map_indexed(|x, index| x + (index as i64));",
        "xs.rank_by(|x| order::desc(x));",
        "xs.group_by(|x| { key: x % 2 });",
        "bucket.key();",
        "bucket.items();",
        "xs.index_by(|x| x.id);",
        "xs.key_by(|x| x.id);",
        "xs.associate_by(|x| x.id, |x| x.name);",
        "xs.fold(0, |acc, x| acc + x);",
        "xs.reduce(|acc, x| acc + x);",
        "xs.scan(0, |state, x| { left: state + x, right: state + x });",
        "xs.contains(needle);",
        "xs.contains_by(|x| x.id == needle);",
        "xs.partition(|x| x.active);",
        "xs.distinct();",
        "xs.distinct_by(|x| x.id);",
        "xs.union(ys);",
        "xs.intersect(ys);",
        "xs.except(ys);",
        "xs.join_by(ys, |x, y| x.id == y.id);",
        "xs.left_join_by(ys, |x, y| x.id == y.id);",
        "xs.semi_join_by(ys, |x, y| x.id == y.id);",
        "xs.anti_join_by(ys, |x, y| x.id == y.id);",
        "xs.zip(ys);",
        "graph::transitive_closure({ seed: xs, step: |x| [x + 1], key: |x| x });",
    ] {
        parse_all::<Stmt>(src);
    }
}

#[test]
fn native_collection_path_selector_surface_parses_without_extra_map_selector() {
    for src in [
        "users@u[order by u.id].id;",
        "users@u[order by u.id].{ id: u.id, name: u.name };",
        "users@u[order by u.id][1..3].id;",
        "users@u[order by u.id][1..=3].{ id: u.id };",
        "users@u[group by { city: u.city, team: u.team }];",
        "users@u[enumerate];",
        "users@u[distinct];",
        "users@u[distinct by u.city];",
        "users@u[*].friends@f[**].id;",
        "users@u[*].friends@f[***].{ id: f.id };",
    ] {
        parse_all::<Stmt>(src);
    }
}

#[test]
fn native_collection_record_destructuring_surface_parses_in_decl_and_assignment_positions() {
    for src in [
        "let { index, value: user, .. } = users.enumerate().at(0);",
        "{ let mut existing_index = 0; let mut existing_user = users.at(0); { index: existing_index, value: existing_user, .. } = users.enumerate().at(0); };",
        "for { index, value: user } in users.enumerate() { total = total + (index as i64); };",
        "match users.enumerate().at(0) { { index, value: user } => user.id };",
        "users.enumerate().map(|{ index, value: user }| user.id + (index as i64));",
    ] {
        parse_all::<Stmt>(src);
    }
}

#[test]
fn native_collection_query_tail_surface_parses_range_and_root_local_modifiers() {
    for src in [
        "select users@u[order by u.id].id from users@u:User range 0..10",
        "select users@u[order by u.id].id from users@u:User range 0..=9",
        "select users@u[order by u.id].id from (users@u:User where u.active order by u.id asc range ..10)",
        "select groups@g[*].{ key: g.key(), count: g.items().count() } from users@u:User group by { city: u.city } into groups",
        "select users@u[*].follows@e[**].friends@f[**].id from users@u:User links (users)->[follows@e:Follows hops 1..=3]->(friends@f:User)",
    ] {
        parse_all::<crate::query::SelectQ>(src);
    }
}
