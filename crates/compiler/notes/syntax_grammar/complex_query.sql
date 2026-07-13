
(there may be some logical/semantic mistakes in the query i have provided. so its more of a guideline than 100% correct truth)
// note: bare selector indexing like blogs[0] is now read as arbitrary engine-chosen order.
// add [order by ...][0] if you need deterministic nth semantics.

let user_age = 120;

select users@u1[where u1.name == 'ye'].{
  name:
  user_id: u1.id,
  age: user_age,
  old_blog_names: u1.blogs@b1[where b1.date < dt'01-01-2023'].name,
  new_blog_names: u1.blogs@b2[where b2.date > dt'01-01-2023'].name,
  blog_2024: u1.blogs@b3[where b3.date == date_2024][0:45].info[*].tags@t[where t.kind == 'tech' and t.name == u1.name].name,
  old_blog_name: u1.blogs[0].name,
    another: (select u1.blogs@blg1[*].{
                name,
                nana:   select book[*].name from (books@b1:Book where b1.views > u1.age),
                } 
               from (blog:Blog where blg1.views > user_age)),

  score: {
        let name = 'biggie';
        let   score = user_age * 5;
        len(name) + score + u1.blogs[0].user_count
  },
  blogs: u1.blogs@blg[where blg.title == 'cool' and u1.age > 50].{ title, extra: 'some stuff' }
}
from (users@u1:User where u1.age > 50)
LINKS
  (users) -> [write@w:UserWritesBlog WHERE w.published_date > date_2024] -> (blogs@b:Blog where b.views > w.score)
  <- [read_by : UserReadsBlog] <- (young_users@yu:User where yu.age < 20 and yu.age > u1.age and len(users) > 10),
(users@u2 where u2.friend_count > 3) -> [performs@p: UserPerformsAtEvent where p.attendance > w.count] 
where u1.age > 30 and u1.write[*].blogs[**].any(|b| b.title == 'something') and len(u1.write[*].blogs[**]) > 5
// group by { age: u1.age, uname_alias: u1.name } into groups
// planned: projection can be e.g groups@g[*].{ age: g.age, uname_alias: g.uname_alias, total_users: count(g.users@u[*]) }


u1.writes@w[*].book@b[**].{
    name,
    date_written: w.date_written,
    written_by: w.written_by,
    book_id: b.id,
    book_name: b.name,
    book_author: b.author,
    book_published_date: b.published_date,
    book_tags: b.tags@t[where t.kind == 'tech' and t.name == u1.name].name,
}

---

SELECT users@u[where u.name == 'ye'].{
  name:
  user_id: u.id,
  age: user_age,
  old_blog_names: u.blogs@b1[where b1.date < dt'01-01-2023'].name,
  new_blog_names: u.blogs@b2[where b2.date > dt'01-01-2023'].name,
  blog_2024: u.blogs@b3[where b3.date == date_2024][0:45].info[*].tags@t[where t.kind == 'tech' and t.name == u.name].name,
  old_blog_name: u.blogs[0].name,
    another: (select blogs@blg1[*].{
                name,
                nana:   select books@bk[*].name from (books@bk:Book where bk.views > u.age),
                } 
               from (blogs@bl:Blog where bl.views > user_age)),

  score: {
        let name = 'biggie';
        let   score = user_age * 5;
        len(name) + score + b3[0].user_count
  },
  blogs: blog@blg[where blg.title == 'cool' and u.age > 50].{ title, extra: 'some stuff' }
}
FROM (users@u:User where u.age > 50)
LINKS
  (users) -> [writes@w:UserWritesBlog WHERE w.published_date > date_2024] -> (blogs@b:Blog WHERE b.views > w.score)
  <- [read_by : UserReadsBlog] <- (young_users@yu: User where yu.age < 20 and yu.age > users.age),
(users@u2 where u2.friend_count > 3) -> [performs@p: UserPerformsAtEvent where p.attendance > w.count]
WHERE u.age > 30 AND u.writes[*].blogs[**].any(|b| b.title == 'something') and len(u.writes[*].blogs[**]) > 5
