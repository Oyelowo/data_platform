use super::harness::*;

#[test]
fn test_query_select_simple() {
    assert_snapshot("select_simple", "select name from user;");
}
#[test]
fn test_query_select_complex() {
    let input = "
        select users@u[*].{
            name, 
            count: math::count(u.posts),
        }
        from user@u:User 
        where u.age > 18 
        order by u.name asc;
    ";
    assert_snapshot("select_complex", input);
    // Also round-trip it to ensure we can regenerate valid SQL
    assert_round_trip::<Stmt>(input);
}
#[test]
fn test_query_filtering() {
    assert_round_trip::<Stmt>("select users[*] from user@u:User where u.age > 18;");
}
#[test]
fn test_graph_links() {
    let input = "
            link (user:User) -> [follows@f:UserFollowsUser {
                since: now(),
                mutual: false
            }] -> (target:User),

            (user) -> [follows@f:UserFollowsUser {
                since: now(),
                mutual: false
            }] -> (target:User)

            return user[WHERE user.age > 20].{
                id,
                name: concat(user.name, 4)
            }
        ";

    assert_round_trip::<Stmt>(input);
    // assert_round_trip::<Stmt>("link (a) -> [e@e] (b);");
    // assert_round_trip::<Stmt>("link (a) -> [e@e] (b);");
    // assert_round_trip::<Stmt>("link (a) -> [es@e:EdgeType where e.weight > 5] (b);");
}
#[test]
fn test_modifications() {
    // CREATE
    assert_round_trip::<Stmt>("create user@u:User { name: 'John', age: 30 };");
    // UPDATE with SET
    assert_round_trip::<Stmt>("update users@u:User set u.name = 'Jane' where u.id == 1;");
    // UPDATE with CONTENT
    assert_round_trip::<Stmt>("update users@u:User { name: 'Jane' } where u.id == 1;");
    // DELETE
    assert_round_trip::<Stmt>("delete users@u:User where u.age < 18;");
}
