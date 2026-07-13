#[cfg(test)]
#[allow(unused)]
mod test {

    use super::super::*;
    use crate::Interner;
    use crate::tokenizer::TokenKind;
    use yelang_lexer::{All, TokenError, TokenStream, TokenizeChars};

    #[test]
    fn test_select_statement() {
        let _input1 = "select users@u[*].{
            name,
            age,
        } from users:User;";
        let _input = "
select user@u[*].{
  user_id: u.id,
  name,
  age,
  blogs_with_writes_details: u.write@w[*].blogs[**].{
    date: w.published_date,
    title
  },
  blogs: u.blogs[*].{
    title
  },
}
from users@u:User
links
  (users) -> [write@w1:UserWritesBlog WHERE w1.published_date > dt'2020-01-01'] -> (blog@b:Blog WHERE b.views > 10000)
   <- [likes:UserLikesBlog] <- (other_users:User),
  (users) -> [write@w2:WritesBlog WHERE w2.published_date > dt'2020-01-01'] -> (blog@b2:Blog WHERE b2.views > 10000),
where users[0..6].age[2] > 30 and u.age > 20
// ;

";
        let _ = (_input1, _input);
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::Interner;
    use crate::tokenizer::TokenKind;
    use yelang_lexer::{All, TokenError};

    #[test]
    fn test_hop_range_parses_dotdot_and_dotdoteq() {
        let mut interner = Interner::new();

        let mut stream = TokenKind::tokenize("1..3", &mut interner).unwrap();
        let hops = stream.parse::<All<HopRange>>().unwrap().into_inner();
        assert!(!hops.inclusive);

        let mut stream = TokenKind::tokenize("1..=3", &mut interner).unwrap();
        let hops = stream.parse::<All<HopRange>>().unwrap().into_inner();
        assert!(hops.inclusive);
    }

    #[test]
    fn test_links_hops_keyword_parses_bounded_traversal_range() {
        let mut interner = Interner::new();
        let input = "select 1 from users@u:User links (users)->[follows@e:Follows hops 1..=3]->(friends@f:User)";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let select = stream.parse::<All<SelectQ>>().unwrap().into_inner();

        let hops = select.links[0].segments[0]
            .edge
            .hops
            .as_ref()
            .expect("expected edge hop range");
        assert!(hops.start.is_some());
        assert!(hops.end.is_some());
        assert!(hops.inclusive);
    }

    #[test]
    fn test_links_reject_bare_hop_range_without_hops_keyword() {
        let mut interner = Interner::new();
        let input =
            "select 1 from users@u:User links (users)->[follows@e:Follows 1..=3]->(friends@f:User)";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let err = stream.parse::<SelectQ>().unwrap_err();

        assert!(
            err.to_string().contains("1"),
            "expected bare hop range to be rejected, got {err:?}"
        );
    }

    #[test]
    fn test_select_pipeline_range_parses_range_expr_shapes() {
        let mut interner = Interner::new();

        let cases = [
            ("range 1..3", true, true, false),
            ("range ..3", false, true, false),
            ("range 1..", true, false, false),
            ("range 1..=3", true, true, true),
        ];

        for (src, has_start, has_end, inclusive) in cases {
            let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
            let range = stream.parse::<All<Range>>().unwrap().into_inner();
            assert_eq!(
                range.start.is_some(),
                has_start,
                "unexpected start for {src}"
            );
            assert_eq!(range.end.is_some(), has_end, "unexpected end for {src}");
            assert_eq!(
                range.inclusive, inclusive,
                "unexpected inclusivity for {src}"
            );
        }
    }

    #[test]
    fn test_select_tail_range_parses_in_query_and_from_modifiers() {
        let mut interner = Interner::new();

        let mut stream = TokenKind::tokenize(
            "select users@u[*].id from (users@u:User order by u.id asc range 1..3) range ..10",
            &mut interner,
        )
        .unwrap();
        let select = stream.parse::<All<SelectQ>>().unwrap().into_inner();

        let from_range = select.from[0]
            .modifiers
            .range
            .as_ref()
            .expect("expected FROM modifier range");
        assert!(from_range.start.is_some());
        assert!(from_range.end.is_some());
        assert!(!from_range.inclusive);

        let tail_range = select.range.as_ref().expect("expected select tail range");
        assert!(tail_range.start.is_none());
        assert!(tail_range.end.is_some());
    }

    #[test]
    fn test_select_projection_chains_selector_after_order_by() {
        let mut interner = Interner::new();

        for input in [
            "select users@u[order by u.id].id from users@u:User",
            "select users@u[order by u.id].{ id: u.id } from users@u:User",
            "select users@u[order by u.id][*].id from users@u:User",
            "select users@u[order by u.id][*].{ id: u.id } from users@u:User",
        ] {
            let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
            let select = stream.parse::<All<SelectQ>>().unwrap().into_inner();

            assert_eq!(select.from.len(), 1, "failed to parse `{input}`");
        }
    }

    #[test]
    fn test_post_links_for_blocks_parse_range_modifiers() {
        let mut interner = Interner::new();

        let input = r#"
            select {
                users: users@u[*].id,
                books: books@b[*].id,
            }
            from users@u:User, books@b:Book
            links (users)->[writes@w:UserWritesBook]->(books)
            for users { order by u.id asc range ..10 }
            for books { where b.genre == "Tech" order by b.id desc range 0..5 }
        "#;

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let select = stream.parse::<All<SelectQ>>().unwrap().into_inner();

        assert_eq!(select.post_links_for.len(), 2);
        assert!(select.post_links_for[0].modifiers.range.is_some());
        assert!(select.post_links_for[1].modifiers.range.is_some());
    }

    #[test]
    fn test_select_tail_rejects_legacy_start_limit_range() {
        let mut interner = Interner::new();
        let input = "select users@u[*].id from users@u:User limit 1 start 1";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let err = stream.parse::<All<SelectQ>>().unwrap_err();

        match err {
            TokenError::CustomError { msg, .. } => assert!(
                msg.contains("start") && msg.contains("limit"),
                "expected explicit legacy range-tail rejection, got {msg}"
            ),
            other => panic!("expected explicit legacy range-tail rejection, got {other:?}"),
        }
    }

    #[test]
    fn test_group_by_object_keys_parse() {
        let mut interner = Interner::new();
        let input = "group by { is_adult: u.age > min_age, city: u.addr.city } into groups";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let group_by = stream.parse::<All<GroupByClause>>().unwrap().into_inner();

        assert_eq!(group_by.keys.len(), 2);

        assert_eq!(
            group_by.keys[0].name.as_ref().unwrap().as_str(&interner),
            "is_adult"
        );
        assert_eq!(
            group_by.keys[1].name.as_ref().unwrap().as_str(&interner),
            "city"
        );

        assert_eq!(group_by.into.as_str(&interner), "groups");
    }

    #[test]
    fn test_group_by_nested_object_key_value_parses_as_object() {
        let mut interner = Interner::new();
        let input = "group by { stats: { city: u.city } } into groups";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let group_by = stream.parse::<All<GroupByClause>>().unwrap().into_inner();

        assert_eq!(group_by.keys.len(), 1);
        assert_eq!(
            group_by.keys[0].name.as_ref().unwrap().as_str(&interner),
            "stats"
        );
        assert!(
            matches!(group_by.keys[0].expr.kind, crate::ExprKind::Object(_)),
            "expected nested GROUP BY value to parse as object, got {:?}",
            group_by.keys[0].expr.kind
        );
    }

    #[test]
    fn test_group_by_requires_into() {
        let mut interner = Interner::new();
        let input = "group by { city: u.city }";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let err = stream.parse::<All<GroupByClause>>().unwrap_err();

        let TokenError::CustomError { msg, .. } = err else {
            panic!("expected CustomError, got {err:?}");
        };

        assert!(
            msg.contains("requires an `into <label>`"),
            "unexpected msg: {msg}"
        );
    }

    #[test]
    fn test_group_by_requires_object_keys() {
        let mut interner = Interner::new();
        let input = "group by u.city into groups";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let err = stream.parse::<All<GroupByClause>>().unwrap_err();

        let TokenError::CustomError { msg, .. } = err else {
            panic!("expected CustomError, got {err:?}");
        };

        assert!(
            msg.contains("requires object keys"),
            "unexpected msg: {msg}"
        );
    }

    #[test]
    fn test_links_inner_parses_required_match_kind() {
        let mut interner = Interner::new();
        let input = "select 1 from users@u:User links inner (users)->[follows@f:UserFollowsUser]->(targets@t:User)";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let select = stream.parse::<All<SelectQ>>().unwrap().into_inner();

        assert_eq!(select.links_match_kind, LinksMatchKind::Required);
        assert_eq!(select.links.len(), 1);
    }

    #[test]
    fn test_from_modifiers_require_parentheses() {
        let mut interner = Interner::new();
        let input = "select 1 from (users@u:User where true) where false";
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let select = stream.parse::<All<SelectQ>>().unwrap().into_inner();

        assert_eq!(select.from.len(), 1);
        assert!(select.from[0].modifiers.filter.is_some());
        assert!(select.where_clause.is_some());
    }

    #[test]
    fn test_select_requires_from_clause() {
        let mut interner = Interner::new();
        let input = "select 1";

        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let err = stream.parse::<All<SelectQ>>().unwrap_err();

        let TokenError::CustomError { msg, .. } = err else {
            panic!("expected CustomError, got {err:?}");
        };

        assert!(msg.contains("SELECT requires a `from` clause"));
    }

    #[test]
    fn test_links_after_tail_emits_hint() {
        let mut interner = Interner::new();
        let input = "select 1 from users@u:User where true links (users)";
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let err = stream.parse::<All<SelectQ>>().unwrap_err();

        let TokenError::CustomError { msg, .. } = err else {
            panic!("expected CustomError, got {err:?}");
        };

        assert!(msg.contains("wrap the FROM source in parentheses"));
        assert!(msg.contains("FROM modifiers require parentheses"));
    }
}
